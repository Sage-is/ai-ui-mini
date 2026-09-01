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

// Where payload files can live, most specific first.
//
// The cask ships a self-contained .app, so everything the shell needs — engine,
// studio template, sandbox profile, product marker — sits in Contents/Resources
// and moves with the bundle. The walk-up entries keep the older Homebrew
// libexec layout working, where those files are siblings of the .app.
//
// Always derived from current_exe(), canonicalized: a Finder launch has cwd `/`,
// and the app is normally reached through a symlink, so anything cwd-relative or
// un-resolved finds nothing.
fn payload_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(me) = std::env::current_exe().map(|p| p.canonicalize().unwrap_or(p)) {
        if let Some(dir) = me.parent() {
            roots.push(dir.to_path_buf());
            roots.push(dir.join("../Resources"));
            let mut up = dir.to_path_buf();
            for _ in 0..6 {
                roots.push(up.clone());
                if !up.pop() {
                    break;
                }
            }
        }
    }
    roots
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

    for root in payload_roots() {
        cands.push(root.join(exe));
        cands.push(root.join("bin").join(exe));
        cands.push(root.join(engine_target()).join("bin").join(exe));
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
    payload_roots()
        .into_iter()
        .map(|r| r.join("studio"))
        .find(|c| c.join("opencode.json").is_file())
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
        // No curriculum template: this is the bare platform. Do not invent a
        // courses/ folder, which is a Downes concept -- but do not ship with
        // no config at all either. Without one the engine falls back to its
        // own defaults and asks before every edit, which is the permission
        // prompting teachers reported. Deny reaching outside the workspace and
        // leave `edit` to the default: mini has no folder convention to
        // allow-list, so a blanket allow here could not be written safely
        // (`**` matches `../.ssh/id_rsa`).
        let cfg = studio.join("opencode.json");
        if !cfg.exists() {
            let _ = fs::write(
                &cfg,
                "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"permission\": {\n    \"external_directory\": {\n      \"*\": \"deny\"\n    }\n  }\n}\n",
            );
        }
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
    for c in payload_roots().into_iter().map(|r| r.join("product")) {
        if let Ok(s) = fs::read_to_string(&c) {
            let name = s.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    // No marker: a developer build. `tauri build` emits the bundle into
    // target/release/bundle/macos with nothing staged around it, so the walk
    // above finds nothing and mini would call itself Downes and write into
    // ~/Downes — the wrong name and the wrong workspace, on the machine most
    // likely to be demoing it. Fall back to the bundle identifier, which both
    // products always carry and which the two tauri configs already differ on.
    if bundle_identifier().as_deref() == Some("is.sage.mini") {
        return "SAGE.ISmini".into();
    }
    "Downes".into()
}

// CFBundleIdentifier out of the app's own Info.plist. Scanned as text rather
// than parsed: this is a one-key lookup on a file we ship ourselves, and it
// must not add a plist dependency to the shell.
fn bundle_identifier() -> Option<String> {
    let me = std::env::current_exe().ok()?;
    // …/Foo.app/Contents/MacOS/exe → …/Foo.app/Contents/Info.plist
    let plist = me.parent()?.parent()?.join("Info.plist");
    let text = fs::read_to_string(plist).ok()?;
    let after_key = &text[text.find("<key>CFBundleIdentifier</key>")?..];
    let start = after_key.find("<string>")? + "<string>".len();
    let end = after_key[start..].find("</string>")?;
    Some(after_key[start..start + end].trim().to_string())
}

// The same marker, as the name a user should see. The workspace folder is
// "SAGE.ISmini" with no space, because a space in a home-directory path is a
// nuisance at a shell; the product is written "mini".
fn product_label() -> String {
    match product_workspace().as_str() {
        "SAGE.ISmini" | "SageMini" => "mini".into(),
        other => other.to_string(),
    }
}

fn studio_dir() -> PathBuf {
    let dir = std::env::var("DOWNES_STUDIO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(product_workspace()));
    adopt_old_workspace(&dir);
    dir
}

// mini's workspace was "~/SageMini" through 0.1.7. Renaming it without moving
// anything would strand a user's work in a folder the app no longer opens, so
// carry it over once. Only when the new name does not exist yet: if both are
// present the user has already been here, and merging two workspaces on a
// guess is not ours to do.
fn adopt_old_workspace(dir: &Path) {
    if dir.file_name().and_then(|n| n.to_str()) != Some("SAGE.ISmini") || dir.exists() {
        return;
    }
    let old = home().join("SageMini");
    if old.is_dir() {
        let _ = fs::rename(&old, dir);
    }
}

// The fork's opencode package (run from source in dev).
// Kill the sidecar AND the engine under it.
//
// The handle we hold is `sandbox-exec`; the engine is its child and puts
// itself in a fresh process group, so neither `child.kill()` nor a kill on our
// own group reaches it. Through 0.1.6 that left `opencode serve` listening on
// 127.0.0.1 after the window closed, one orphan per launch. Take the children
// by parent pid first, then the wrapper, then reap so nothing is left a zombie.
fn reap(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/usr/bin/pkill")
            .arg("-P")
            .arg(child.id().to_string())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn fork_opencode() -> PathBuf {
    if let Ok(p) = std::env::var("DOWNES_FORK") {
        return PathBuf::from(p);
    }
    // Walk up from the BINARY, not the working directory. current_dir() is
    // whatever shell or Finder happened to launch us from, so on a dev machine
    // this probed ~/Documents/... one level at a time -- a tree the sandbox
    // profile explicitly denies. An installed app has no fork above it, so the
    // walk simply finds nothing and stops, which is the correct answer.
    //
    // This is the same rule as payload_roots() and studio_template(): derive
    // from current_exe(), never from the working directory.
    let mut p = std::env::current_exe()
        .map(|e| e.canonicalize().unwrap_or(e))
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    for _ in 0..6 {
        let cand = p.join("packages/opencode/src/index.ts");
        if cand.exists() {
            return p.join("packages/opencode");
        }
        if !p.pop() {
            break;
        }
    }
    // No fallback. This used to name the maintainer's own checkout, which
    // shipped that path to every user and could only ever resolve on one Mac.
    // An empty path fails the `is_file()` checks downstream, which is the
    // honest answer when there is no fork here.
    PathBuf::new()
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
// launcher/downes.sh does the same for the terminal. The two entry points must
// stay in step — same XDG roots, same skill flags, same DOWNES_* switches — or
// the isolation is half a fix and the fence covers whichever surface nobody
// uses. They differ in exactly one place, deliberately: how each refuses an
// incoherent flag combination, below.
//
// DOWNES_SHARE_STATE=1 opts back out to the shared home store — but only with
// the fence off, the same pairing rule launcher/downes.sh enforces. Sharing
// state puts auth.json, the database and the log back under
// ~/.local/share/opencode, which downes.sb does not make writable; honouring
// the flag while fenced hands the user an engine that dies opening its own log.
//
// The launcher refuses that combination outright. A GUI has no terminal to
// refuse into and a blank window is worse than a working one, so the studio
// keeps isolation instead and records the decision in sidecar.log. Returns the
// lines to log.
fn isolate_state(cmd: &mut Command, studio: &Path) -> Vec<String> {
    let share = std::env::var("DOWNES_SHARE_STATE").as_deref() == Ok("1");
    let fence_off = std::env::var("DOWNES_NO_SANDBOX").as_deref() == Ok("1");
    if share && fence_off {
        return vec![
            "downes: sharing the home state store (DOWNES_SHARE_STATE=1, fence off)".into(),
        ];
    }
    let note: String = if share {
        "downes: ignoring DOWNES_SHARE_STATE=1 — it needs DOWNES_NO_SANDBOX=1, \
         and shared state is unwritable inside the fence"
            .into()
    } else {
        "downes: state isolated to the studio".into()
    };
    let mut notes = vec![note];
    let xdg = studio.join(".downes").join("xdg");

    // Our state must never be committable. The studio is the folder teachers
    // are told to keep courses in and share with colleagues, and the seed below
    // puts real provider keys inside it — a `git add -A` in ~/Downes would
    // otherwise stage them.
    let gitignore = studio.join(".gitignore");
    let already = fs::read_to_string(&gitignore).unwrap_or_default();
    if !already.lines().any(|l| l.trim() == ".downes/xdg/") {
        use std::io::Write;
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore)
        {
            let _ = writeln!(f, ".downes/xdg/");
        }
    }
    for (var, sub) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        let dir = xdg.join(sub);
        let _ = fs::create_dir_all(&dir);

        // The engine's folder inside each XDG root is named after its
        // compiled-in channel (core/src/global.ts): "opencode" before v0.1.4,
        // "downes" from here on. Bring an existing studio's state with us, or
        // the teacher opens the studio to an empty session list.
        let (old, new) = (dir.join("opencode"), dir.join("downes"));
        if old.is_dir() && !new.exists() {
            let _ = fs::rename(&old, &new);
        }

        cmd.env(var, &dir);
    }

    // Seed the credential store once from the user's real opencode, so
    // isolation costs nobody a second login. The two diverge after this: a
    // provider added here will not show up in a stock opencode session.
    let seed = xdg.join("data").join("downes").join("auth.json");
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

    // Now that the studio owns a data directory, pull back anything an older
    // build left in the user's shared opencode folder.
    notes.extend(reclaim_shared_store(&xdg.join("data").join("downes")));
    notes
}

// Builds before v0.1.3 had no XDG isolation, so our engine wrote its database
// into ~/.local/share/opencode beside a stock opencode's own — two databases,
// two schemas, one directory, and an error message naming neither. That cost a
// colleague a morning, and `brew uninstall` leaves it behind.
//
// Ours is identifiable: the engine names the database after its compiled-in
// channel (core/src/database/database.ts), which for this fork is downes/v1 →
// opencode-downes-v1.db. Stock opencode ships channel latest/beta/prod and uses
// the unsuffixed opencode.db. We must never touch that one.
//
// launcher/downes.sh does the same for the terminal. This exists because a
// teacher who only ever double-clicks would otherwise never run it.
// Sessions held in a database, read through sqlite3 with immutable=1 so a live
// writer is never disturbed and no WAL is created. None when unreadable, which
// the caller treats as "do not touch".
fn session_count(db: &Path) -> Option<u64> {
    let out = Command::new("/usr/bin/sqlite3")
        .arg(format!("file:{}?immutable=1", db.display()))
        .arg("select count(*) from session;")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn reclaim_shared_store(mine: &Path) -> Vec<String> {
    let shared = home().join(".local/share/opencode");
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(&shared) else {
        return notes;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("opencode-downes-") && name.ends_with(".db")) {
            continue;
        }
        let src = shared.join(&name);

        // Never move a database out from under a live process. Same guard the
        // launcher uses; macOS always ships lsof.
        let busy = Command::new("/usr/sbin/lsof")
            .arg("--")
            .arg(&src)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if busy {
            notes.push(format!("downes: {name} is in use; left in the shared store"));
            continue;
        }

        let _ = fs::create_dir_all(mine);
        let sidecars = [format!("{name}-shm"), format!("{name}-wal")];

        if mine.join(&name).exists() {
            // Both exist. Decide on CONTENT, never on which file happens to be
            // present: isolation shipped a release before this reclaim did, so
            // the studio's copy is usually the newer-but-emptier one, and
            // picking it silently retires a term of a teacher's sessions.
            let theirs = session_count(&src);
            let ours = session_count(&mine.join(&name));
            match (theirs, ours) {
                (Some(t), Some(o)) if t > o => {
                    // The stray is richer. Park the sparse copy, never delete.
                    if fs::rename(&mine.join(&name), mine.join(format!("{name}.sparse"))).is_ok() {
                        for s in &sidecars {
                            let _ = fs::remove_file(mine.join(s));
                        }
                        if fs::rename(&src, mine.join(&name)).is_ok() {
                            for s in &sidecars {
                                let _ = fs::rename(shared.join(s), mine.join(s));
                            }
                            notes.push(format!(
                                "downes: recovered {t} sessions an older build left in your \
                                 opencode folder; the studio's {o}-session copy is kept as \
                                 {name}.sparse"
                            ));
                        }
                    }
                }
                _ => {
                    // Ours is as rich or richer, or neither could be read. Take
                    // the stray out of the shared folder but keep it — deleting
                    // a teacher's only copy on a guess is not ours to do.
                    if fs::rename(&src, mine.join(format!("{name}.from-shared-store"))).is_ok() {
                        for s in &sidecars {
                            let _ = fs::remove_file(shared.join(s));
                        }
                        notes.push(format!(
                            "downes: an older build left {name} in your opencode folder; \
                             moved it into the studio as {name}.from-shared-store"
                        ));
                    }
                }
            }
        } else if fs::rename(&src, mine.join(&name)).is_ok() {
            for s in &sidecars {
                let _ = fs::rename(shared.join(s), mine.join(s));
            }
            notes.push(format!(
                "downes: moved {name} out of ~/.local/share/opencode into the studio"
            ));
        }
    }
    notes
}

// launcher/downes.sb, found the same way engine_bin() finds the engine: walk
// up from the canonicalized exe. Payload layout is libexec/launcher/downes.sb
// beside libexec/<Product>.app, so the profile travels with every install.
fn sandbox_profile() -> Option<PathBuf> {
    payload_roots()
        .into_iter()
        .map(|r| r.join("launcher").join("downes.sb"))
        .find(|p| p.is_file())
}

// The studio is the product's main surface, so leaving it unfenced while the
// terminal launcher fences would make "works in one folder" true only for the
// command nobody runs. Same profile, same params, same bypass switch as
// launcher/downes.sh.
//
// STUDIO and TMP must be PHYSICAL paths: the sandbox canonicalizes before
// matching a subpath rule, so /var/folders/... or a symlinked studio never
// matches and the allow is silently dead.
fn sandbox_prefix(studio: &Path) -> Option<(PathBuf, Vec<String>)> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    if std::env::var("DOWNES_NO_SANDBOX").as_deref() == Ok("1") {
        return None;
    }
    let profile = sandbox_profile()?;
    let studio_phys = studio.canonicalize().unwrap_or_else(|_| studio.to_path_buf());
    let tmp = std::env::temp_dir();
    let tmp_phys = tmp.canonicalize().unwrap_or(tmp);
    let home = home();
    Some((
        PathBuf::from("/usr/bin/sandbox-exec"),
        vec![
            "-D".into(),
            format!("STUDIO={}", studio_phys.display()),
            "-D".into(),
            format!("TMP={}", tmp_phys.display()),
            "-D".into(),
            format!("HOMEDIR={}", home.display()),
            "-f".into(),
            profile.display().to_string(),
        ],
    ))
}

fn spawn_sidecar(studio: &Path, port: u16, password: &str) -> Option<Child> {
    let fork = fork_opencode();
    let port_s = port.to_string();

    // Prefer the compiled fork binary: it needs no runtime on PATH and idles
    // near 0% CPU. Fall back to running from source under an absolutely
    // resolved bun (dev machines that have not built the binary yet).
    let bin = engine_bin(&fork);
    // Layer 3. Built here rather than at each call site so the from-source
    // fallback below is fenced too — an unfenced dev path is how a fence stops
    // being tested.
    let fence = sandbox_prefix(studio);
    let mut cmd = if !bin.is_empty() {
        let mut c = match &fence {
            Some((sbx, args)) => {
                let mut c = Command::new(sbx);
                c.args(args).arg(&bin);
                c
            }
            None => Command::new(&bin),
        };
        c.args(["serve", "--hostname", "127.0.0.1", "--port", &port_s]);
        // The compiled engine is self-contained and must NOT be run from the
        // fork: on an installed copy that source tree does not exist, and
        // spawning with a missing cwd fails outright. The studio is the
        // correct working directory anyway — it is the project.
        c.current_dir(studio);
        c
    } else {
        // No compiled engine in this bundle. That is normal for a dev build run
        // from the source tree, and a bug anywhere else: `tauri build` emits a
        // bundle into target/release/bundle/macos with nothing staged around
        // it, Spotlight indexes it, and clicking it lands here. Say so, because
        // the symptom otherwise reaches the user as "sidecar unreachable" with
        // nothing naming the cause.
        if fork.as_os_str().is_empty() {
            eprintln!(
                "downes: this bundle carries no engine and no source tree is above it.\n                   It is an incomplete `tauri build` artifact, not an installed app.\n                   Install with `brew install sage-is/apps/mini`, or set DOWNES_ENGINE."
            );
            return None;
        }
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

    let state_notes = isolate_state(&mut cmd, studio);

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

        // The model pin and public-tier key travel by env, not in
        // $STUDIO/opencode.json — that file sits at the root of a folder the
        // teacher opens in other tools, and opencode adopts any opencode.json
        // it finds walking up from its working directory. Leaving model,
        // small_model or the provider key in it silently repoints a stock
        // opencode user's account and billing. Merged last, so it wins.
        // launcher/downes.sh carries the same pair.
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            r#"{"model":"opencode/nemotron-3.5-lightning-free","small_model":"opencode/big-pickle","provider":{"opencode":{"options":{"apiKey":"public"}}}}"#,
        );
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
        Ok((mut out, err)) => {
            // Say which of the two happened, in the file people actually read.
            // A fence that fails open in silence is worse than no fence: the
            // copy still claims containment and nothing contradicts it.
            use std::io::Write;
            for n in &state_notes {
                let _ = writeln!(out, "{n}");
            }
            let _ = match (&fence, std::env::var("DOWNES_NO_SANDBOX").as_deref()) {
                (Some((_, a)), _) => {
                    let p = a.last().map(String::as_str).unwrap_or("?");
                    writeln!(out, "downes: sandboxed with {p}")
                }
                (None, Ok("1")) => writeln!(out, "downes: sandbox bypassed (DOWNES_NO_SANDBOX=1)"),
                (None, _) if cfg!(target_os = "macos") => writeln!(
                    out,
                    "downes: WARNING running UNFENCED — launcher/downes.sb not found beside the app"
                ),
                (None, _) => Ok(()),
            };
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

    // Stand in our own workspace before anything else runs. Launched from a
    // terminal, the app inherits that shell's directory; launched from Finder
    // it inherits `/`. Either way it is not ours, and the sandbox profile
    // denies ~/Documents, ~/Desktop and ~/Downloads outright -- so a path
    // derived from an inherited cwd is at best wrong and at worst a denial the
    // user sees as "sidecar unreachable". The studio is the project; make it
    // the working directory too.
    let _ = std::env::set_current_dir(&studio);

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
                            reap(&mut child);
                        }
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        // Window-destroyed alone is not enough: Cmd-Q and the app menu can end
        // the process without it, which is one route to the orphaned engine.
        // Exit fires for every route, and reap() is safe to run twice.
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut c) = state.child.lock() {
                        if let Some(mut child) = c.take() {
                            reap(&mut child);
                        }
                    }
                }
            }
        });
}

// ---- tests -----------------------------------------------------------------
// The import rules are the studio's edge: they decide what arbitrary dropped
// content is allowed to become. Worth testing rather than eyeballing.
#[cfg(test)]
mod tests {
    use super::*;

    // The profile lives at libexec/launcher/downes.sb and the app at
    // libexec/<Product>.app, so the walk from Contents/MacOS has to survive
    // three pops. Restructure the payload and the fence fails open in
    // silence — this is the assertion that turns that into a red test.
    #[test]
    fn finds_the_profile_across_the_payload_layout() {
        let root = tmp("profile-walk");
        let exe_dir = root.join("libexec/Sage.is mini.app/Contents/MacOS");
        fs::create_dir_all(&exe_dir).unwrap();
        let launcher = root.join("libexec/launcher");
        fs::create_dir_all(&launcher).unwrap();
        fs::write(launcher.join("downes.sb"), "(version 1)").unwrap();

        // Same walk sandbox_profile() performs, against a real payload shape.
        let mut up = exe_dir.clone();
        let mut hit = None;
        for _ in 0..6 {
            let p = up.join("launcher").join("downes.sb");
            if p.is_file() {
                hit = Some(p);
                break;
            }
            if !up.pop() {
                break;
            }
        }
        assert_eq!(hit, Some(launcher.join("downes.sb")));

        // And a tree without one must not invent a fence.
        let bare = tmp("profile-walk-bare");
        let bare_exe = bare.join("target/release/bundle/macos/X.app/Contents/MacOS");
        fs::create_dir_all(&bare_exe).unwrap();
        let mut up = bare_exe;
        let mut hit2 = None;
        for _ in 0..6 {
            let p = up.join("launcher").join("downes.sb");
            if p.is_file() {
                hit2 = Some(p);
                break;
            }
            if !up.pop() {
                break;
            }
        }
        assert_eq!(hit2, None);
    }

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
