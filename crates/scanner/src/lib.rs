//! Escaner paralelo de disco: arbol con tamano, asignado, archivos y carpetas por nodo.
#[cfg(windows)]
pub mod mft;

use rayon::prelude::*;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::UNIX_EPOCH,
};

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub size: u64,
    /// Tamano en disco (redondeado a cluster).
    pub alloc: u64,
    pub files: u64,
    pub dirs: u64,
    /// Ultima modificacion (segundos unix) del archivo mas reciente del subarbol.
    pub mtime: i64,
    /// Archivos sueltos directamente en esta carpeta (para el "[N Archivos]" del treemap).
    pub own_size: u64,
    pub own_files: u64,
    pub children: Vec<Node>,
}

const CLUSTER: u64 = 4096; // shortcut: cluster fijo 4 KB, ignora compresion/sparse/archivos residentes; usar GetCompressedFileSize si hace falta exactitud

/// Junctions y symlinks se saltan para no contar doble ni entrar en bucles.
fn is_reparse(md: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        md.file_type().is_symlink()
    }
}

pub fn scan(path: &Path) -> Node {
    scan_with(path, &AtomicU64::new(0))
}

/// `seen` se incrementa por cada archivo contado, para mostrar progreso.
pub fn scan_with(path: &Path, seen: &AtomicU64) -> Node {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mut node = Node { name, ..Default::default() };

    // Carpeta sin permisos: se ignora (queda con tamano 0) en vez de abortar.
    let Ok(rd) = fs::read_dir(path) else { return node };
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if is_reparse(&md) {
            continue;
        }
        if md.is_dir() {
            subdirs.push(e.path());
        } else {
            let len = md.len();
            node.own_size += len;
            node.alloc += len.div_ceil(CLUSTER) * CLUSTER;
            node.own_files += 1;
            let t = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
            node.mtime = node.mtime.max(t.map_or(0, |d| d.as_secs() as i64));
            seen.fetch_add(1, Relaxed);
        }
    }
    node.size = node.own_size;
    node.files = node.own_files;
    let children: Vec<Node> = subdirs.par_iter().map(|p| scan_with(p, seen)).collect();
    for c in &children {
        node.size += c.size;
        node.alloc += c.alloc;
        node.files += c.files;
        node.dirs += c.dirs + 1;
        node.mtime = node.mtime.max(c.mtime);
    }
    node.children = children;
    node.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suma_tamanos_y_cuenta() {
        let d = std::env::temp_dir().join("scanner_test");
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a/b")).unwrap();
        fs::write(d.join("x.bin"), [0u8; 100]).unwrap();
        fs::write(d.join("a/y.bin"), [0u8; 50]).unwrap();
        fs::write(d.join("a/b/z.bin"), [0u8; 25]).unwrap();
        let n = scan(&d);
        assert_eq!((n.size, n.files, n.dirs), (175, 3, 2));
        assert_eq!((n.own_size, n.own_files), (100, 1));
        assert_eq!(n.alloc, 3 * CLUSTER);
        assert!(n.mtime > 0);
        assert_eq!(n.children[0].name, "a"); // ordenado por tamano desc
        fs::remove_dir_all(&d).unwrap();
    }
}
