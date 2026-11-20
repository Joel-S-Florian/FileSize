use std::{path::PathBuf, sync::atomic::AtomicU64, time::Instant};

fn fmt(b: u64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (b as f64, 0);
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    format!("{v:.1} {}", u[i])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let turbo = args.iter().any(|a| a == "--mft");
    let path: PathBuf = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "C:\\".into()).into();
    let t = Instant::now();
    let n = if turbo {
        let d = path.to_string_lossy().chars().next().unwrap_or('C');
        scanner::mft::scan_mft(d, &AtomicU64::new(0)).expect("fallo el modo MFT (necesita administrador y NTFS)")
    } else {
        scanner::scan(&path)
    };
    let el = t.elapsed();
    println!("{} [{}]: {} | asignado {} | {} archivos | {} carpetas | {:.2?}", path.display(), if turbo { "MFT" } else { "normal" }, fmt(n.size), fmt(n.alloc), n.files, n.dirs, el);
    for c in n.children.iter().take(12) {
        println!("{:>10}  {:>5.1}%  {} ({} arch.)", fmt(c.size), c.size as f64 * 100.0 / n.size.max(1) as f64, c.name, c.files);
    }
}
