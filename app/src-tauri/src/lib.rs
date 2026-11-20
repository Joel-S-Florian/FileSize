use scanner::Node;
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

/// El arbol completo vive en Rust; la UI solo pide los hijos que muestra.
#[derive(Default)]
struct AppState(Mutex<Option<Node>>);

#[derive(Serialize)]
struct Row {
    name: String,
    size: u64,
    alloc: u64,
    files: u64,
    dirs: u64,
    mtime: i64,
    has_children: bool,
    /// Posicion en `children` del padre (para armar la ruta); None en "[N Archivos]".
    idx: Option<usize>,
}

fn row(n: &Node, idx: Option<usize>) -> Row {
    Row {
        name: n.name.clone(),
        size: n.size,
        alloc: n.alloc,
        files: n.files,
        dirs: n.dirs,
        mtime: n.mtime,
        has_children: !n.children.is_empty() || n.own_files > 0,
        idx,
    }
}

fn files_label(n: u64) -> String {
    format!("[{n} Archivos]")
}

fn at<'a>(root: &'a Node, path: &[usize]) -> Option<&'a Node> {
    path.iter().try_fold(root, |n, &i| n.children.get(i))
}

#[cfg(windows)]
fn free_space(path: &str) -> Option<u64> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let w: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
    // SAFETY: puntero a cadena terminada en 0 y tres u64 validos.
    (unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) } != 0).then_some(f)
}
#[cfg(not(windows))]
fn free_space(_: &str) -> Option<u64> {
    None
}

#[derive(Serialize)]
struct ScanResult {
    root: Row,
    /// Solo cuando se escanea la raiz de un disco.
    free: Option<u64>,
    /// "mft" o "normal".
    mode: &'static str,
    /// Aviso cuando se pidio turbo y hubo que caer al escaneo normal.
    note: Option<String>,
}

#[cfg(windows)]
fn scan_mft(path: &str, seen: &AtomicU64) -> Result<Node, String> {
    if !scanner::mft::is_elevated() {
        return Err("requiere ejecutar como administrador".into());
    }
    scanner::mft::scan_mft(path.chars().next().unwrap_or('C'), seen).map_err(|e| e.to_string())
}
#[cfg(not(windows))]
fn scan_mft(_: &str, _: &AtomicU64) -> Result<Node, String> {
    Err("solo disponible en Windows".into())
}

#[tauri::command]
fn is_admin() -> bool {
    #[cfg(windows)]
    return scanner::mft::is_elevated();
    #[cfg(not(windows))]
    false
}

/// Abre otra instancia con UAC. En release cierra esta; en dev la deja viva porque el servidor vite depende de ella.
#[tauri::command]
fn relaunch_admin(app: AppHandle) -> Result<(), String> {
    // ShellExecuteW "runas" dispara el UAC sin depender de que powershell este en el PATH.
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(hwnd: isize, verb: *const u16, file: *const u16, args: *const u16, dir: *const u16, show: i32) -> isize;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let (verb, file) = (wide("runas"), wide(&exe.display().to_string()));
    // SAFETY: cadenas terminadas en 0 que viven durante la llamada; el resto son nulos permitidos.
    let r = unsafe { ShellExecuteW(0, verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    if r <= 32 {
        return Err("Se canceló la elevación".into()); // 5 = el usuario rechazo el UAC
    }
    if !cfg!(debug_assertions) {
        app.exit(0);
    }
    Ok(())
}

#[derive(Serialize)]
struct TNode {
    name: String,
    size: u64,
    path: Vec<usize>,
    loose: bool,
    kids: Vec<TNode>,
}

fn metric_of(n: &Node, metric: &str) -> u64 {
    match metric {
        "alloc" => n.alloc,
        "files" => n.files,
        _ => n.size,
    }
}

fn tnode(n: &Node, path: Vec<usize>, depth: u32, min: u64, metric: &str) -> TNode {
    let mut kids = Vec::new();
    if depth > 0 {
        let mut idx: Vec<usize> = (0..n.children.len()).filter(|&i| metric_of(&n.children[i], metric) >= min).collect();
        idx.sort_by_key(|&i| std::cmp::Reverse(metric_of(&n.children[i], metric)));
        for i in idx.into_iter().take(40) {
            let mut p = path.clone();
            p.push(i);
            kids.push(tnode(&n.children[i], p, depth - 1, min, metric));
        }
        let own = if metric == "files" { n.own_files } else { n.own_size };
        if own >= min && own > 0 {
            kids.push(TNode { name: files_label(n.own_files), size: own, path: path.clone(), loose: true, kids: vec![] });
            kids.sort_by(|a, b| b.size.cmp(&a.size));
        }
    }
    TNode { name: n.name.clone(), size: metric_of(n, metric), path, loose: false, kids }
}

#[tauri::command]
fn drives() -> Vec<String> {
    ('A'..='Z').map(|c| format!("{c}:\\")).filter(|d| Path::new(d).exists()).collect()
}

#[tauri::command(async)]
fn scan(app: AppHandle, state: State<AppState>, path: String, turbo: bool) -> Result<ScanResult, String> {
    if !Path::new(&path).is_dir() {
        return Err(format!("No es una carpeta: {path}"));
    }
    let seen = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let ticker = {
        let (seen, done) = (seen.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Relaxed) {
                let _ = app.emit("scan-progress", seen.load(Relaxed));
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let is_root = path.len() <= 3;
    let (mut mode, mut note) = ("normal", None);
    let mut root = None;
    if turbo && is_root {
        match scan_mft(&path, &seen) {
            Ok(n) => (root, mode) = (Some(n), "mft"),
            Err(e) => note = Some(format!("Modo turbo no disponible ({e}); se uso el escaneo normal.")),
        }
        seen.store(0, Relaxed);
    }
    let root = root.unwrap_or_else(|| scanner::scan_with(Path::new(&path), &seen));
    done.store(true, Relaxed);
    let _ = ticker.join();
    let free = if is_root { free_space(&path) } else { None };
    let r = row(&root, None);
    *state.0.lock().unwrap() = Some(root);
    Ok(ScanResult { root: r, free, mode, note })
}

/// `path` = indices de hijo desde la raiz hasta el nodo pedido.
#[tauri::command]
fn children(state: State<AppState>, path: Vec<usize>) -> Vec<Row> {
    let g = state.0.lock().unwrap();
    let Some(n) = g.as_ref().and_then(|r| at(r, &path)) else { return vec![] };
    let mut rows: Vec<Row> = n.children.iter().enumerate().map(|(i, c)| row(c, Some(i))).collect();
    if n.own_files > 0 {
        rows.push(Row {
            name: files_label(n.own_files),
            size: n.own_size,
            alloc: n.own_size.div_ceil(4096) * 4096, // aprox; el real vive en cada archivo
            files: n.own_files,
            dirs: 0,
            mtime: 0,
            has_children: false,
            idx: None,
        });
        rows.sort_by(|a, b| b.size.cmp(&a.size));
    }
    rows
}

/// Subarbol recortado para dibujar el treemap (maximo 4 niveles, 40 hijos, sin rectangulos diminutos).
#[tauri::command]
fn treemap(state: State<AppState>, path: Vec<usize>, metric: String) -> Option<TNode> {
    let g = state.0.lock().unwrap();
    let n = g.as_ref().and_then(|r| at(r, &path))?;
    Some(tnode(n, path, 4, metric_of(n, &metric) / 2000, &metric))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // Mica: el fondo de la ventana toma el color del escritorio, como las apps nativas de Windows 11.
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    let _ = window_vibrancy::apply_mica(&w, None); // falla en Windows 10: queda fondo liso
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![drives, scan, children, treemap, is_admin, relaunch_admin])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
