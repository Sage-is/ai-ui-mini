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
    product: String, // display name for the UI: "Downes" or "mini"
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

// The same marker, as the name a user should see. The workspace folder is
// "SageMini" because a space in a home-directory path is a nuisance; the
// product is written "mini".
fn product_label() -> String {
    match product_workspace().as_str() {
        "SageMini" => "mini".into(),
        other => other.to_string(),
    }
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

// Every product's state resolves through xdg-basedir under a hardcoded app
// name of "opencode" (packages/core/src/global.ts). Left alone, Downes, mini
// and a stock opencode install share one auth.json, one opencode.db and one
// lockfile — last writer wins. Pointing XDG at the studio gives each product
// its own, and is also what makes Layer-3 possible: downes.sb permits writes
// only under STUDIO, so state in ~/.local/share is denied the moment the
// sandbox is switched on.
//
// launcher/downes.sh does exactly this for the TUI. Both entry points must
// agree, or the isolation is half a fix.
//
// DOWNES_SHARE_STATE=1 opts back out to the shared home store.
fn isolate_state(cmd: &mut Command, studio: &Path) {
    if std::env::var("DOWNES_SHARE_STATE").as_deref() == Ok("1") {
        return;
    }
    let xdg = studio.join(".downes").join("xdg");
    for (var, sub) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        let dir = xdg.join(sub);
        let _ = fs::create_dir_all(&dir);
        cmd.env(var, &dir);
    }

    // Seed the credential store once from the user's real opencode, so
    // isolation costs nobody a second login. The two diverge after this: a
    // provider added here will not show up in a stock opencode session.
    let seed = xdg.join("data").join("opencode").join("auth.json");
    let real = home().join(".local/share/opencode/auth.json");
    if !seed.exists() && real.is_file() {
        if let Some(parent) = seed.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(&real, &seed).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&seed, fs::Permissions::from_mode(0o600));
            }
        }
    }
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

    isolate_state(&mut cmd, studio);

    // OPENCODE_CONFIG merges our skills/agent/METHOD; project config stays
    // off so a downloaded course cannot smuggle its own config.
    //
    // The last two keep the machine owner's personal Claude Code setup out of
    // a teacher's session. XDG isolation closes neither channel — both resolve
    // against the real home directory:
    //   - skills: ~/.claude/skills, ~/.agents/skills (skill/index.ts:186)
    //   - prompt: ~/.claude/CLAUDE.md, and a project CLAUDE.md that a
    //     downloaded course could ship (session/instruction.ts:62,66)
    cmd.env("OPENCODE_SERVER_PASSWORD", password)
        .env("OPENCODE_DISABLE_PROJECT_CONFIG", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1")
        .env("OPENCODE_DISABLE_CLAUDE_CODE", "1");

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

// ---- importing files dropped onto the sidebar ------------------------------
//
// The webview hands us absolute OS paths from the drop. Copying them in is the
// one place where content the teacher did not author enters the studio, so the
// rules below are deliberately strict and deliberately visible: what lands is
// exactly what the file manager will show, and nothing already there is lost.

// What one drop did, so the UI can say it plainly instead of flashing a tick.
#[derive(Serialize, Default)]
struct ImportReport {
    imported: usize,
    skipped_hidden: usize,
    skipped_links: usize,
    renamed: usize,
}

// Ceilings for a single drop. A course folder is normal; a home directory is a
// misdrop, and refusing it up front beats discovering it 40,000 files in.
const MAX_FILES: usize = 2000;
const MAX_BYTES: u64 = 500 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

// One file the scan decided to copy: where it is, and where it goes under the
// destination folder.
#[derive(Debug)]
struct Planned {
    from: PathBuf,
    rel: PathBuf,
}

#[derive(Debug, Default)]
struct Scan {
    files: Vec<Planned>,
    dirs: Vec<PathBuf>,
    bytes: u64,
    skipped_hidden: usize,
    skipped_links: usize,
}

// Same rule list_dir hides by, applied on the way in. Importing a file the
// sidebar will never show would leave the teacher with content they cannot see
// and the agent can still read.
fn hidden_name(name: &str) -> bool {
    name.starts_with('.') || HIDDEN.contains(&name)
}

// Walk a dropped path and record what *would* be copied. Nothing is written
// here: a drop that trips a ceiling must leave the studio untouched rather
// than half-populated.
//
// Symlinks are skipped outright, never followed — one rule doing two jobs. It
// stops a cycle (`ln -s .. loop`) from recursing forever, and it stops a link
// aimed at ~/.ssh from planting a live pointer inside the fence. MAX_DEPTH is
// the backstop for anything that nests pathologically without a symlink.
fn scan_source(src: &Path, rel: PathBuf, depth: usize, out: &mut Scan) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("that folder nests deeper than {MAX_DEPTH} levels"));
    }
    let meta = fs::symlink_metadata(src).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        out.skipped_links += 1;
        return Ok(());
    }
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if hidden_name(&name) {
        out.skipped_hidden += 1;
        return Ok(());
    }
    if meta.is_dir() {
        // Recorded so an empty folder still arrives; a course with an unused
        // images/ should not quietly lose it.
        out.dirs.push(rel.clone());
        for e in fs::read_dir(src).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            scan_source(&e.path(), rel.join(e.file_name()), depth + 1, out)?;
        }
        return Ok(());
    }
    out.bytes += meta.len();
    out.files.push(Planned {
        from: src.to_path_buf(),
        rel,
    });
    if out.files.len() > MAX_FILES {
        return Err(format!("that drop holds more than {MAX_FILES} files"));
    }
    if out.bytes > MAX_BYTES {
        return Err(format!(
            "that drop holds more than {} MB",
            MAX_BYTES / 1024 / 1024
        ));
    }
    Ok(())
}

// Never overwrite. A second notes.pdf becomes "notes (2).pdf"; a second
// "Grade 9" folder becomes "Grade 9 (2)". Merging into an existing folder is
// the one outcome a teacher cannot undo by deleting what just arrived, so a
// dropped folder is renamed whole rather than merged file by file.
//
// `claimed` covers names taken earlier in this same drop but not yet written.
fn unique_name(dir: &Path, name: &str, claimed: &[String]) -> Result<(String, bool), String> {
    let taken = |c: &str| dir.join(c).exists() || claimed.iter().any(|k| k == c);
    if !taken(name) {
        return Ok((name.to_string(), false));
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2..1000 {
        let cand = format!("{stem} ({n}){ext}");
        if !taken(&cand) {
            return Ok((cand, true));
        }
    }
    Err(format!("there are already too many copies of {name} here"))
}

#[tauri::command]
fn import_paths(
    state: State<AppState>,
    dest: String,
    sources: Vec<String>,
) -> Result<ImportReport, String> {
    let root = PathBuf::from(&state.server.studio);
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    let dest = root.join(dest.trim_start_matches('/'));
    let dest_canon = dest.canonicalize().map_err(|e| e.to_string())?;
    if !dest_canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    if !dest_canon.is_dir() {
        return Err("that drop target is not a folder".into());
    }

    // Sources are absolute OS paths and are deliberately NOT fenced — reaching
    // outside the studio is the entire point of an import. The destination is
    // fenced, which is the check that actually matters.
    let mut scan = Scan::default();
    let mut claimed: Vec<String> = vec![];
    let mut renamed = 0usize;

    for s in &sources {
        let src = PathBuf::from(s);
        let meta = fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            scan.skipped_links += 1;
            continue;
        }
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            return Err("that drop has no file name".into());
        }
        if hidden_name(&name) {
            scan.skipped_hidden += 1;
            continue;
        }
        let src_canon = src.canonicalize().map_err(|e| e.to_string())?;
        // Copying a folder into itself, or into its own descendant, never ends.
        if dest_canon.starts_with(&src_canon) {
            return Err("a folder cannot be copied into itself".into());
        }
        let (top, was_renamed) = unique_name(&dest_canon, &name, &claimed)?;
        if was_renamed {
            renamed += 1;
        }
        claimed.push(top.clone());

        if meta.is_dir() {
            scan.dirs.push(PathBuf::from(&top));
            for e in fs::read_dir(&src).map_err(|e| e.to_string())? {
                let e = e.map_err(|e| e.to_string())?;
                scan_source(&e.path(), Path::new(&top).join(e.file_name()), 1, &mut scan)?;
            }
        } else {
            scan.bytes += meta.len();
            scan.files.push(Planned {
                from: src.clone(),
                rel: PathBuf::from(&top),
            });
        }
    }

    // Every check has passed; only now does anything get written.
    for d in &scan.dirs {
        fs::create_dir_all(dest_canon.join(d)).map_err(|e| e.to_string())?;
    }
    for f in &scan.files {
        let to = dest_canon.join(&f.rel);
        if let Some(p) = to.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        fs::copy(&f.from, &to).map_err(|e| e.to_string())?;
    }

    Ok(ImportReport {
        imported: scan.files.len(),
        skipped_hidden: scan.skipped_hidden,
        skipped_links: scan.skipped_links,
        renamed,
    })
}

// Show a studio file or folder in the OS file manager, selected and ready to
// be dragged out. This is not the same as dragging from the sidebar: WKWebView
// will not let a web page begin a native file drag, so Finder is the drag
// source instead. Same studio fence as open_in_browser.
#[tauri::command]
fn reveal_in_finder(state: State<AppState>, rel: String) -> Result<(), String> {
    let root = PathBuf::from(&state.server.studio);
    let target = root.join(rel.trim_start_matches('/'));
    let canon = target.canonicalize().map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    if !canon.starts_with(&root_canon) {
        return Err("outside studio".into());
    }
    #[cfg(target_os = "macos")]
    let spawned = Command::new("open").arg("-R").arg(&canon).spawn();
    #[cfg(target_os = "windows")]
    let spawned = Command::new("explorer")
        .arg(format!("/select,{}", canon.display()))
        .spawn();
    // No Linux file manager agrees on a reveal flag, so open the containing
    // folder. The file is not selected — that is the closest honest equivalent.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = Command::new("xdg-open")
        .arg(canon.parent().unwrap_or(&canon))
        .spawn();
    spawned.map(|_| ()).map_err(|e| e.to_string())
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
        product: product_label(),
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
            print_html,
            import_paths,
            reveal_in_finder
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

// ---- tests -----------------------------------------------------------------
// The import rules are the studio's edge: they decide what arbitrary dropped
// content is allowed to become. Worth testing rather than eyeballing.
#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("downes-import-test-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn scan(src: &Path) -> Result<Scan, String> {
        let mut s = Scan::default();
        let rel = PathBuf::from(src.file_name().unwrap());
        scan_source(src, rel, 0, &mut s)?;
        Ok(s)
    }

    #[test]
    fn skips_dotfiles_and_studio_plumbing() {
        let d = tmp("hidden");
        fs::write(d.join("lesson.md"), "x").unwrap();
        fs::write(d.join(".env"), "SECRET=1").unwrap();
        fs::write(d.join("package.json"), "{}").unwrap();

        let s = scan(&d).unwrap();
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.skipped_hidden, 2);
        assert!(s.files[0].rel.ends_with("lesson.md"));
    }

    #[test]
    fn skips_symlinks_without_following_them() {
        let d = tmp("links");
        fs::write(d.join("real.md"), "x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("escape.md")).unwrap();

        let s = scan(&d).unwrap();
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.skipped_links, 1);
        // Nothing outside the dropped folder was even considered.
        assert!(s.files.iter().all(|f| f.from.starts_with(&d)));
    }

    #[test]
    fn symlink_cycle_terminates() {
        let d = tmp("cycle");
        fs::write(d.join("a.md"), "x").unwrap();
        std::os::unix::fs::symlink(&d, d.join("loop")).unwrap();

        // Skipping links rather than following them is what ends this; the
        // depth cap is only the backstop. Either way it must not hang.
        let s = scan(&d).unwrap();
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.skipped_links, 1);
    }

    #[test]
    fn refuses_when_nesting_passes_the_depth_cap() {
        let d = tmp("deep");
        let mut p = d.clone();
        for i in 0..(MAX_DEPTH + 3) {
            p = p.join(format!("d{i}"));
        }
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("deep.md"), "x").unwrap();

        let err = scan(&d).unwrap_err();
        assert!(err.contains("nests deeper"), "unexpected error: {err}");
    }

    #[test]
    fn empty_folders_still_travel() {
        let d = tmp("empty");
        fs::create_dir_all(d.join("images")).unwrap();
        fs::write(d.join("lesson.md"), "x").unwrap();

        let s = scan(&d).unwrap();
        assert!(s.dirs.iter().any(|p| p.ends_with("images")));
    }

    #[test]
    fn renames_rather_than_overwrites() {
        let d = tmp("collide");
        fs::write(d.join("notes.pdf"), "original").unwrap();

        let (name, renamed) = unique_name(&d, "notes.pdf", &[]).unwrap();
        assert_eq!(name, "notes (2).pdf");
        assert!(renamed);

        // A name already claimed earlier in the same drop is taken too, even
        // though nothing has been written to disk for it yet.
        let (next, _) = unique_name(&d, "notes.pdf", &["notes (2).pdf".into()]).unwrap();
        assert_eq!(next, "notes (3).pdf");

        // A folder has no extension to split on.
        fs::create_dir_all(d.join("Grade 9")).unwrap();
        let (dir, _) = unique_name(&d, "Grade 9", &[]).unwrap();
        assert_eq!(dir, "Grade 9 (2)");

        // A free name is left exactly alone.
        let (free, renamed) = unique_name(&d, "fresh.md", &[]).unwrap();
        assert_eq!(free, "fresh.md");
        assert!(!renamed);
    }

    #[test]
    fn dotfile_rule_matches_what_the_sidebar_hides() {
        assert!(hidden_name(".env"));
        assert!(hidden_name("node_modules"));
        assert!(hidden_name("opencode.json"));
        assert!(!hidden_name("lesson.md"));
    }
}
