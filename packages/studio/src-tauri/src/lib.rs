// Downes studio — Tauri shell. Spawns opencode `serve` as a loopback sidecar
// scoped to the studio directory, injects credentials into the webview, and
// exposes studio-fenced file commands for the file manager + artifact viewer.
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};

#[derive(Clone, Serialize)]
struct ServerInfo {
    url: String,
    username: String,
    password: String,
    studio: String,
    fork: String,
}

struct AppState {
    server: ServerInfo,
    child: Mutex<Option<Child>>,
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

fn studio_dir() -> PathBuf {
    std::env::var("DOWNES_STUDIO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join("Downes"))
}

// The fork's opencode package (run from source in dev).
fn fork_opencode() -> PathBuf {
    if let Ok(p) = std::env::var("DOWNES_FORK") {
        return PathBuf::from(p);
    }
    // …/ai-ui-mini/packages/studio/src-tauri → …/ai-ui-mini/packages/opencode
    let mut p = std::env::current_dir().unwrap_or_default();
    for _ in 0..6 {
        let cand = p.join("packages/opencode/src/index.ts");
        if cand.exists() {
            return p.join("packages/opencode");
        }
        if !p.pop() {
            break;
        }
    }
    home().join("Documents/Projects/GitHub/AI-Education-Downes/ai-ui-mini/packages/opencode")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(4096)
}

fn random_password() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

fn spawn_sidecar(studio: &Path, port: u16, password: &str) -> Option<Child> {
    let fork = fork_opencode();
    let d = studio.join(".downes");
    for sub in ["config", "xdg/data", "xdg/state", "xdg/cache"] {
        let _ = fs::create_dir_all(d.join(sub));
    }
    Command::new("bun")
        .args([
            "run",
            "--conditions=browser",
            "src/index.ts",
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&fork)
        .env("OPENCODE_SERVER_PASSWORD", password)
        .env("OPENCODE_CONFIG", studio.join("opencode.json"))
        .env("XDG_CONFIG_HOME", d.join("xdg/config"))
        .env("XDG_DATA_HOME", d.join("xdg/data"))
        .env("XDG_STATE_HOME", d.join("xdg/state"))
        .env("XDG_CACHE_HOME", d.join("xdg/cache"))
        .env("OPENCODE_PURE", "1")
        .env("OPENCODE_DISABLE_PROJECT_CONFIG", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1")
        .env("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS", "1")
        .spawn()
        .ok()
}

#[tauri::command]
fn studio_server(state: State<AppState>) -> ServerInfo {
    state.server.clone()
}

// Read the course tree under the studio, fenced to studio/courses.
#[derive(Serialize)]
struct Node {
    name: String,
    path: String,
    dir: bool,
}

#[tauri::command]
fn list_dir(state: State<AppState>, rel: String) -> Result<Vec<Node>, String> {
    let root = PathBuf::from(&state.server.studio).join("courses");
    let target = root.join(rel.trim_start_matches('/'));
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    if !canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    let mut out = vec![];
    for e in fs::read_dir(&canon).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        out.push(Node {
            name: e.file_name().to_string_lossy().into(),
            path: p
                .strip_prefix(&root_canon)
                .unwrap_or(&p)
                .to_string_lossy()
                .into(),
            dir: p.is_dir(),
        });
    }
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

#[tauri::command]
fn read_file(state: State<AppState>, rel: String) -> Result<String, String> {
    let root = PathBuf::from(&state.server.studio).join("courses");
    let target = root.join(rel.trim_start_matches('/'));
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    if !canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    fs::read_to_string(&canon).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let studio = studio_dir();
    let port = free_port();
    let password = random_password();
    let child = spawn_sidecar(&studio, port, &password);
    let server = ServerInfo {
        url: format!("http://127.0.0.1:{}", port),
        username: "opencode".into(),
        password,
        studio: studio.to_string_lossy().into(),
        fork: fork_opencode().to_string_lossy().into(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            server,
            child: Mutex::new(child),
        })
        .invoke_handler(tauri::generate_handler![
            studio_server,
            list_dir,
            read_file
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if let Ok(mut c) = state.child.lock() {
                        if let Some(mut child) = c.take() {
                            let _ = child.kill();
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
