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
    bin: String, // compiled TUI binary if built, else "" (fall back to source)
}

// Platform/arch fragment matching the names `script/build.ts` emits:
// opencode-darwin-arm64, opencode-linux-x64, opencode-windows-x64, …
fn engine_target() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        other => other,
    };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    format!("opencode-{os}-{arch}")
}

fn engine_exe() -> &'static str {
    if cfg!(target_os = "windows") { "opencode.exe" } else { "opencode" }
}

// Locate the compiled engine.
//
// Resolution is INSTALL-RELATIVE first and developer-checkout last. An
// installed copy must never depend on a source tree existing: under Homebrew
// the payload lands in `libexec/` next to the app, and under a bundled build
// the engine sits beside this executable. Both are derived from
// current_exe(), never from the working directory — a Finder launch has cwd
// `/`, so any cwd-relative search silently finds nothing.
fn engine_bin(fork: &Path) -> String {
    let exe = engine_exe();
    let mut cands: Vec<PathBuf> = Vec::new();

    if let Ok(p) = std::env::var("DOWNES_ENGINE") {
        cands.push(PathBuf::from(p));
    }

    // Canonicalize first. When the app is reached through a symlink — the
    // normal way to get a Homebrew-installed .app into /Applications —
    // current_exe() reports the SYMLINK path, not the real one. Walking up
    // from there finds no libexec, and resolution silently falls through to
    // the developer-checkout candidate below: a false pass on this machine
    // and a dead app on any other.
    if let Ok(me) = std::env::current_exe().map(|p| p.canonicalize().unwrap_or(p)) {
        if let Some(dir) = me.parent() {
            // Tauri externalBin / a plain sibling drop: Contents/MacOS/<exe>
            cands.push(dir.join(exe));
            // Bundled as a resource: Contents/MacOS → Contents/Resources
            cands.push(dir.join("../Resources").join(exe));
            // Homebrew layout: libexec/Downes.app/Contents/MacOS, with the
            // engine at libexec/bin. Walk up rather than counting "..", which
            // is easy to get wrong and fails silently when it is.
            let mut up = dir.to_path_buf();
            for _ in 0..5 {
                cands.push(up.join("bin").join(exe));
                cands.push(up.join(engine_target()).join("bin").join(exe));
                if !up.pop() {
                    break;
                }
            }
        }
    }

    // Developer checkout, last: what `bun run build` produces in the fork.
    cands.push(fork.join("dist").join(engine_target()).join("bin").join(exe));

    cands
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.canonicalize().unwrap_or(p).to_string_lossy().into())
        .unwrap_or_default()
}

// The studio template shipped inside the payload: opencode.json, the .downes
// skills/METHOD/prompts, and the courses README. Found the same way the
// engine is — relative to this executable, never a source checkout.
fn studio_template() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DOWNES_TEMPLATE") {
        let p = PathBuf::from(p);
        if p.join("opencode.json").is_file() {
            return Some(p);
        }
    }
    let me = std::env::current_exe().ok()?;
    let me = me.canonicalize().unwrap_or(me);
    let mut up = me.parent()?.to_path_buf();
    for _ in 0..6 {
        let cand = up.join("studio");
        if cand.join("opencode.json").is_file() {
            return Some(cand);
        }
        if !up.pop() {
            break;
        }
    }
    None
}

fn copy_tree(src: &Path, dst: &Path, overwrite: bool) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to, overwrite)?;
        } else if overwrite || !to.exists() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// Create the studio on first run.
//
// Previously only launcher/downes.sh did this, by shelling out to
// install_studio.sh. Clicking the app skipped it entirely, so on a machine
// where ~/Downes did not already exist the studio came up empty, with no
// README, and OPENCODE_CONFIG pointed at an opencode.json that was never
// written — the engine then had no config and the window reported "sidecar
// unreachable". Invisible on any machine that had already run the launcher.
//
// Done in Rust rather than by calling the shell script: the app must not
// depend on bash, rsync or git being present, and must work when only the
// .app was copied.
//
// Additive, never destructive. Teacher work is never touched: courses/ and
// any existing README are left alone. opencode.json and the .downes engine
// files ARE refreshed, so an upgrade ships new skills and permission fixes.
fn ensure_studio(studio: &Path) {
    // The workspace itself always exists — it is the engine's cwd.
    let _ = fs::create_dir_all(studio);

    let Some(tpl) = studio_template() else {
        // No curriculum template: this is the bare platform. Stop here rather
        // than inventing a courses/ folder, which is a Downes concept.
        return;
    };
    let _ = fs::create_dir_all(studio.join("courses"));

    let cfg = studio.join("opencode.json");
    if let Err(e) = fs::copy(tpl.join("opencode.json"), &cfg) {
        eprintln!("downes: could not write {}: {e}", cfg.display());
    }

    // Engine-owned files: refresh on upgrade.
    let dot = tpl.join(".downes");
    if dot.is_dir() {
        let _ = copy_tree(&dot, &studio.join(".downes"), true);
    }

    // Teacher-facing: seed once, never overwrite.
    let readme = studio.join("courses/README.md");
    if !readme.exists() {
        let _ = fs::copy(tpl.join("courses-README.md"), &readme);
    }
}

struct AppState {
    server: ServerInfo,
    child: Mutex<Option<Child>>,
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

// Which product this bundle is. One Rust binary ships in two apps — Downes
// (curriculum agent, AGPL payload) and Sage.is mini (bare platform, MIT) —
// which differ only in bundle metadata, so the difference cannot come from
// cfg!(). It comes from a marker staged beside the app in the payload.
//
// Absent marker means Downes, so installs that predate this keep working.
fn product_workspace() -> String {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(me) = std::env::current_exe().map(|p| p.canonicalize().unwrap_or(p)) {
        if let Some(dir) = me.parent() {
            let mut up = dir.to_path_buf();
            for _ in 0..6 {
                cands.push(up.join("product"));
                if !up.pop() {
                    break;
                }
            }
        }
    }
    for c in cands {
        if let Ok(s) = fs::read_to_string(&c) {
            let name = s.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    "Downes".into()
}

fn studio_dir() -> PathBuf {
    std::env::var("DOWNES_STUDIO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(product_workspace()))
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

// Locate `bun` by absolute path. A GUI app launched from Finder/Dock does
// NOT inherit the shell PATH — it gets /usr/bin:/bin:/usr/sbin:/sbin — so a
// bare Command::new("bun") resolves to nothing and the sidecar never starts.
// That is invisible in `tauri dev` (which inherits the terminal's PATH) and
// breaks every bundled install.
fn find_bun() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DOWNES_BUN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let mut cands = vec![
        PathBuf::from("/opt/homebrew/bin/bun"),
        PathBuf::from("/usr/local/bin/bun"),
    ];
    cands.push(home().join(".bun/bin/bun"));
    cands.into_iter().find(|p| p.exists())
}

fn spawn_sidecar(studio: &Path, port: u16, password: &str) -> Option<Child> {
    let fork = fork_opencode();
    let port_s = port.to_string();

    // Prefer the compiled fork binary: it needs no runtime on PATH and idles
    // near 0% CPU. Fall back to running from source under an absolutely
    // resolved bun (dev machines that have not built the binary yet).
    let bin = engine_bin(&fork);
    let mut cmd = if !bin.is_empty() {
        let mut c = Command::new(&bin);
        c.args(["serve", "--hostname", "127.0.0.1", "--port", &port_s]);
        // The compiled engine is self-contained and must NOT be run from the
        // fork: on an installed copy that source tree does not exist, and
        // spawning with a missing cwd fails outright. The studio is the
        // correct working directory anyway — it is the project.
        c.current_dir(studio);
        c
    } else {
        let bun = find_bun()?;
        let mut c = Command::new(bun);
        c.args([
            "run",
            "--conditions=browser",
            "src/index.ts",
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port_s,
        ]);
        // Only the from-source path needs the fork as cwd.
        c.current_dir(&fork);
        c
    };

    // Layer the curriculum config on top of the user's normal opencode
    // environment. We deliberately do NOT isolate XDG or use --pure: the
    // studio shares the user's real providers, models, and connections and
    // can save new ones (auth persists globally), matching plain opencode.
    // OPENCODE_CONFIG merges our skills/agent/METHOD; project config stays
    // off so a downloaded course cannot smuggle its own config.
    cmd.env("OPENCODE_SERVER_PASSWORD", password)
        .env("OPENCODE_DISABLE_PROJECT_CONFIG", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1");

    // Only name a config that exists. Downes ships one; the bare Sage.is mini
    // platform does not, and pointing the engine at a missing file makes it
    // complain on every launch.
    let cfg = studio.join("opencode.json");
    if cfg.is_file() {
        cmd.env("OPENCODE_CONFIG", &cfg);
    }

    // Give the child real stdio. A bundle launched from Finder/Dock inherits
    // no usable stdout/stderr, and the sidecar dies the moment it logs its
    // "listening on…" line. Under `tauri dev` it inherits the terminal, so
    // this failure is invisible there and fatal in every shipped install.
    // The log doubles as the first place to look when a studio comes up blank.
    let log_dir = studio.join(".downes");
    let _ = fs::create_dir_all(&log_dir);
    match fs::File::create(log_dir.join("sidecar.log")).and_then(|f| {
        let err = f.try_clone()?;
        Ok((f, err))
    }) {
        Ok((out, err)) => {
            cmd.stdout(out).stderr(err);
        }
        Err(_) => {
            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
        }
    }
    cmd.stdin(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) => {
            // Do not swallow this: a dead sidecar is the difference between a
            // working studio and a blank window.
            eprintln!("downes: failed to start opencode sidecar: {e}");
            None
        }
    }
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

// Studio plumbing the teacher never needs to see in the file manager.
const HIDDEN: &[&str] = &[
    "node_modules",
    "package.json",
    "package-lock.json",
    "bun.lock",
    "opencode.json",
];

#[tauri::command]
fn list_dir(state: State<AppState>, rel: String) -> Result<Vec<Node>, String> {
    // Root at the studio itself, not just courses/, so anything the TUI
    // writes (a course folder OR a stray artifact at the root) is visible.
    let root = PathBuf::from(&state.server.studio);
    let target = root.join(rel.trim_start_matches('/'));
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    if !canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    let mut out = vec![];
    for e in fs::read_dir(&canon).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || HIDDEN.contains(&name.as_str()) {
            continue; // hide dotfiles + studio plumbing
        }
        let p = e.path();
        out.push(Node {
            name: name.clone(),
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

// Open an external http(s) link in the user's default browser. Owned here
// rather than via the opener plugin's scope config — one dependable path.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) links".into());
    }
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };
    Command::new(prog)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// Open a studio file (an HTML artifact) in the user's default browser. Fenced
// to the studio; the real browser gets the raw on-disk file, so its own origin
// makes links, history, and relative assets work natively (no srcdoc bridge).
#[tauri::command]
fn open_in_browser(state: State<AppState>, rel: String) -> Result<(), String> {
    let root = PathBuf::from(&state.server.studio);
    let target = root.join(rel.trim_start_matches('/'));
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    if !canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };
    Command::new(prog)
        .arg(&canon)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// Print / Save as PDF. The macOS webview (WKWebView) does not implement
// JavaScript window.print(), so an in-app print silently no-ops. Instead we
// write the rendered artifact to a temp HTML file and open it in the default
// browser, where its own print dialog (and Save as PDF) works reliably. The
// page auto-fires print on load.
#[tauri::command]
fn print_html(html: String) -> Result<(), String> {
    let mut path = std::env::temp_dir();
    path.push("downes-print.html");
    fs::write(&path, html).map_err(|e| e.to_string())?;
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };
    Command::new(prog)
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn read_file(state: State<AppState>, rel: String) -> Result<String, String> {
    let root = PathBuf::from(&state.server.studio);
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
    // Must run before the sidecar: it writes the opencode.json that
    // OPENCODE_CONFIG points at.
    ensure_studio(&studio);
    let child = spawn_sidecar(&studio, port, &password);
    let server = ServerInfo {
        url: format!("http://127.0.0.1:{}", port),
        username: "opencode".into(),
        password,
        studio: studio.to_string_lossy().into(),
        fork: fork_opencode().to_string_lossy().into(),
        bin: engine_bin(&fork_opencode()),
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
            read_file,
            open_external,
            open_in_browser,
            print_html
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
