//! Escaneo rapido leyendo la MFT de NTFS directamente. Requiere administrador.
use crate::Node;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

unsafe extern "system" {
    fn IsUserAnAdmin() -> i32;
}

pub fn is_elevated() -> bool {
    // SAFETY: funcion sin argumentos ni efectos secundarios.
    unsafe { IsUserAnAdmin() != 0 }
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Restaura los 2 ultimos bytes de cada sector de 512 B (update sequence array).
fn fixup(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let (off, n) = (u16le(rec, 4) as usize, u16le(rec, 6) as usize);
    if n < 1 || off + n * 2 > rec.len() {
        return false;
    }
    let usn = [rec[off], rec[off + 1]];
    for i in 1..n {
        let end = i * 512;
        if end > rec.len() || rec[end - 2..end] != usn {
            return false; // registro a medio escribir
        }
        let (a, b) = (rec[off + 2 * i], rec[off + 2 * i + 1]);
        rec[end - 2] = a;
        rec[end - 1] = b;
    }
    true
}

fn each_attr(rec: &[u8], mut f: impl FnMut(u32, &[u8])) {
    let mut off = u16le(rec, 0x14) as usize;
    while off + 16 <= rec.len() {
        let ty = u32le(rec, off);
        let len = u32le(rec, off + 4) as usize;
        if ty == 0xFFFF_FFFF || len < 16 || off + len > rec.len() {
            break;
        }
        f(ty, &rec[off..off + len]);
        off += len;
    }
}

struct Rec {
    /// Un padre por cada hardlink (WinSxS depende de esto); las carpetas tienen uno solo.
    parents: Vec<u64>,
    name: Option<String>, // solo carpetas: de los archivos solo importa el tamano
    dir: bool,
    reparse: bool,
    /// Registro de extension: lleva el $DATA de otro archivo (`ext_of` = su registro base).
    ext_of: u64,
    has_list: bool,
    has_data: bool,
    size: u64,
    alloc: u64,
    mtime: i64,
}

fn parse(rec: &mut [u8]) -> Option<Rec> {
    if !fixup(rec) {
        return None;
    }
    let flags = u16le(rec, 0x16);
    if flags & 1 == 0 {
        return None;
    }
    let ext_of = u64le(rec, 0x20) & 0xFFFF_FFFF_FFFF;
    let dir = flags & 2 != 0;
    let mut r = Rec {
        parents: vec![], name: None, dir, reparse: false, ext_of, has_list: false, has_data: false,
        size: 0, alloc: 0, mtime: 0,
    };
    let mut names: Vec<(u8, u64, Option<String>)> = vec![];
    each_attr(rec, |ty, a| {
        let nonres = a[8] != 0;
        match ty {
            0x20 => r.has_list = true,
            0x10 if !nonres && ext_of == 0 => {
                let c = u16le(a, 0x14) as usize;
                if c + 0x24 <= a.len() {
                    let t = u64le(a, c + 8);
                    r.mtime = if t == 0 { 0 } else { (t / 10_000_000) as i64 - 11_644_473_600 };
                    r.reparse = u32le(a, c + 0x20) & 0x400 != 0;
                }
            }
            0x30 if !nonres && ext_of == 0 => {
                let c = u16le(a, 0x14) as usize;
                if c + 0x42 > a.len() {
                    return;
                }
                let mut name = None;
                if dir {
                    let n = a[c + 0x40] as usize;
                    if c + 0x42 + n * 2 <= a.len() {
                        let u: Vec<u16> = (0..n).map(|i| u16le(a, c + 0x42 + i * 2)).collect();
                        name = Some(String::from_utf16_lossy(&u));
                    }
                }
                names.push((a[c + 0x41], u64le(a, c) & 0xFFFF_FFFF_FFFF, name));
            }
            0x80 if a[9] == 0 => {
                if !nonres {
                    if ext_of == 0 {
                        r.size = u32le(a, 0x10) as u64; // residente: vive dentro de la MFT, sin clusters
                    }
                } else if a.len() >= 0x48 && u64le(a, 0x10) == 0 {
                    r.has_data = true;
                    r.size = u64le(a, 0x30);
                    r.alloc = u64le(a, 0x28);
                    if u16le(a, 0x0C) & 0x8001 != 0 {
                        r.alloc = u64le(a, 0x40); // comprimido/disperso: tamano real en disco
                    }
                }
            }
            _ => {}
        }
    });
    if ext_of != 0 {
        return r.has_data.then_some(r);
    }
    // nombres 8.3 (namespace 2) solo cuentan si no hay otro: Win32+DOS son el mismo enlace
    let long = names.iter().any(|n| n.0 != 2);
    for (ns, parent, name) in names {
        if long && ns == 2 {
            continue;
        }
        if r.name.is_none() {
            r.name = name;
        }
        r.parents.push(parent);
    }
    if r.dir {
        r.parents.truncate(1);
    }
    (!r.parents.is_empty()).then_some(r)
}

/// Decodifica un runlist de datos: (lcn inicial, longitud en clusters).
fn runs(a: &[u8], mut p: usize) -> Vec<(u64, u64)> {
    let (mut out, mut lcn) = (vec![], 0i64);
    while p < a.len() && a[p] != 0 {
        let (ln, lo) = ((a[p] & 0xF) as usize, (a[p] >> 4) as usize);
        p += 1;
        if p + ln + lo > a.len() {
            break;
        }
        let len = a[p..p + ln].iter().rev().fold(0u64, |v, &b| v << 8 | b as u64);
        let mut off = a[p + ln..p + ln + lo].iter().rev().fold(0i64, |v, &b| v << 8 | b as i64);
        if lo > 0 && a[p + ln + lo - 1] & 0x80 != 0 {
            off -= 1i64 << (8 * lo); // signo
        }
        lcn += off;
        out.push((lcn as u64, len));
        p += ln + lo;
    }
    out
}

fn read_at(f: &mut File, off: u64, buf: &mut [u8]) -> io::Result<()> {
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(buf)
}

#[derive(Default, Clone, Copy)]
struct Acc {
    size: u64,
    alloc: u64,
    files: u64,
    mtime: i64,
}

struct Dir {
    recno: u64,
    name: String,
    reparse: bool,
}

pub fn scan_mft(drive: char, seen: &AtomicU64) -> io::Result<Node> {
    let mut f = File::open(format!(r"\\.\{drive}:"))?;
    let mut boot = vec![0u8; 4096]; // los volumenes exigen lecturas alineadas a sector
    read_at(&mut f, 0, &mut boot)?;
    if &boot[3..7] != b"NTFS" {
        return Err(bad("el volumen no es NTFS"));
    }
    let cl = u16le(&boot, 0x0B) as u64 * boot[0x0D] as u64;
    let c = boot[0x40] as i8;
    let rs = if c > 0 { c as u64 * cl } else { 1u64 << -c } as usize;
    let mut r0 = vec![0u8; rs.max(4096)];
    read_at(&mut f, u64le(&boot, 0x30) * cl, &mut r0)?;
    r0.truncate(rs);
    if !fixup(&mut r0) {
        return Err(bad("registro $MFT corrupto"));
    }
    let (mut total, mut extents) = (0u64, vec![]);
    each_attr(&r0, |ty, a| {
        if ty == 0x80 && a[8] != 0 && a[9] == 0 && a.len() >= 0x40 {
            total = u64le(a, 0x30) / rs as u64;
            extents = runs(a, u16le(a, 0x20) as usize);
        }
    });
    if total == 0 {
        return Err(bad("no se encontro el $DATA de la MFT"));
    }

    let mut acc = vec![Acc::default(); total as usize];
    let mut dirs: Vec<Dir> = vec![];
    let mut kids: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut with_list: HashMap<u64, Vec<u64>> = HashMap::new(); // base -> padres, para sumarle sus extensiones
    let mut exts: Vec<(u64, u64, u64)> = vec![];
    let mut buf = vec![0u8; 16 << 20];
    let mut recno = 0u64;
    'all: for (lcn, len) in extents {
        let (mut pos, end) = (lcn * cl, (lcn + len) * cl);
        while pos < end {
            let n = ((end - pos) as usize).min(buf.len());
            read_at(&mut f, pos, &mut buf[..n])?;
            let base = recno;
            let parsed: Vec<(u64, Rec)> = buf[..n]
                .par_chunks_mut(rs)
                .enumerate()
                .filter_map(|(i, rec)| parse(rec).map(|r| (base + i as u64, r)))
                .collect();
            for (no, r) in parsed {
                if no >= total {
                    break 'all;
                }
                if r.ext_of != 0 {
                    exts.push((r.ext_of, r.size, r.alloc));
                } else if r.dir {
                    kids.entry(r.parents[0]).or_default().push(dirs.len());
                    dirs.push(Dir { recno: no, name: r.name.unwrap_or_default(), reparse: r.reparse });
                    acc[no as usize].mtime = r.mtime;
                } else {
                    for &p in &r.parents {
                        let a = &mut acc[p.min(total - 1) as usize];
                        a.size += r.size;
                        a.alloc += r.alloc;
                        a.files += 1;
                        a.mtime = a.mtime.max(r.mtime);
                    }
                    seen.fetch_add(1, Relaxed);
                    if r.has_list {
                        with_list.insert(no, r.parents);
                    }
                }
            }
            recno += (n / rs) as u64;
            pos += n as u64;
        }
    }

    for (base, size, alloc) in exts {
        for &p in with_list.get(&base).into_iter().flatten() {
            let a = &mut acc[p.min(total - 1) as usize];
            a.size += size;
            a.alloc += alloc;
        }
    }

    let mut root = build(5, format!("{drive}:\\"), &dirs, &kids, &acc);
    root.name = format!("{drive}:\\");
    Ok(root)
}

fn build(rec: u64, name: String, dirs: &[Dir], kids: &HashMap<u64, Vec<usize>>, acc: &[Acc]) -> Node {
    let a = acc[rec as usize];
    let mut n = Node {
        name,
        size: a.size,
        alloc: a.alloc,
        files: a.files,
        mtime: a.mtime,
        own_size: a.size,
        own_files: a.files,
        ..Default::default()
    };
    for &i in kids.get(&rec).map(|v| v.as_slice()).unwrap_or(&[]) {
        let d = &dirs[i];
        if d.reparse || d.recno == rec {
            continue; // junctions/symlinks, y la raiz que se tiene a si misma de padre
        }
        let c = build(d.recno, d.name.clone(), dirs, kids, acc);
        n.size += c.size;
        n.alloc += c.alloc;
        n.files += c.files;
        n.dirs += c.dirs + 1;
        n.mtime = n.mtime.max(c.mtime);
        n.children.push(c);
    }
    n.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodifica_runlist_con_salto_negativo() {
        // 0x21: len 1 byte, offset 2 bytes. Run 1: 0x18 clusters en 0x5634; run 2: 4 clusters en 0x5634-0x10.
        let a = [0x21, 0x18, 0x34, 0x56, 0x11, 0x04, 0xF0, 0x00];
        assert_eq!(runs(&a, 0), vec![(0x5634, 0x18), (0x5624, 4)]);
    }
}
