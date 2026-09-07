# Porting SAGE.IS mini to Windows

## Scope

macOS on Apple Silicon is the only build that ships. This document lists what a Windows port must do. 

## Already done — do not redo

The Tauri shell already branches per platform in several places. Do not rewrite these:

- `packages/studio/src-tauri/src/lib.rs:38` — `engine_exe()` returns `opencode.exe` on Windows.
- `packages/studio/src-tauri/src/lib.rs:1132` — the Reveal command uses `explorer /select,` on Windows.
- `packages/studio/src-tauri/src/lib.rs:833-883` — `open_external` and its neighbours already branch three ways. The Windows branch is wrong; see Blockers.
- `packages/studio/src-tauri/src/lib.rs:594` — `sandbox_prefix()` returns `None` off macOS. That is intentional; see Containment below.
- `packages/opencode/script/build.ts` already lists `win32` targets for both `x64` and `arm64`, and sets the embedded worker's bunfs root to `B:/~BUN/root/` for Windows builds.
- `icons-mini/` already holds `icon.ico` and the `Square*Logo.png` set that Windows packaging needs.

## Build

Engine:

```bash
bun run --cwd packages/opencode build
```

Output lands in `dist/opencode-windows-<arch>/bin/`.

Shell:

```bash
cd packages/studio && bunx tauri dev
```

Requires bun, the Rust MSVC toolchain, and WebView2.

## Packaging — unwritten

The macOS packaging script lives in the AGPL `AI-Education-Downes` repo and is macOS-only. It cannot simply be translated; a Windows equivalent does not exist yet.

`tauri.conf.json` bundle targets are currently `app` and `dmg`. 

Windows likely needs `nsis` or `msi` added.

A Windows payload must place three things: the compiled engine; a `product` marker file naming the workspace; and `LICENSE-AnnotationMono.txt`, which OFL 1.1 clause 2 requires to travel with the font.

mini must NOT ship the studio curriculum template. That template is AGPL Downes content, and bundling it would put AGPL material inside an MIT artifact.

## Blockers

| File:line | What breaks | Why |
|---|---|---|
| `lib.rs:333` | `random_password()` silently returns an all-zero password | Reads `/dev/urandom`; that open always fails on Windows, and failure is not treated as an error |
| `lib.rs:506` | `reclaim_shared_store()` treats a live database as free to move | Probes `/usr/sbin/lsof` for a live writer and reads probe failure as "not busy" |
| `lib.rs:833-883` | Every "open in browser" action fails on Windows | The three branches pass `start` to `Command::new`, but `start` is a `cmd.exe` builtin and no `start.exe` exists on PATH |
| `lib.rs:282` | `reap()` leaks the engine process on window close | The `pkill` call that reaches the grandchild is `#[cfg(unix)]` only |
| `lib.rs:480` | `session_count()` returns `None` off macOS | Hardcodes `/usr/bin/sqlite3`; callers read `None` as "do not touch", so store reclaim never runs — degrades safely, but silently |
| `lib.rs:450` | Seeded `auth.json` ships world-readable | The `0600` chmod on the seeded credential file is `#[cfg(unix)]` only |
| `lib.rs:346` | Dev-only, not a shipping blocker | `find_bun()` candidate paths (`/opt/homebrew`, `/usr/local`, `~/.bun`) are Unix paths; only the dev source fallback needs bun, since the shipped app uses the compiled engine |
| `lib.rs:52` | None | `payload_roots()` includes `../Resources`, a macOS bundle shape, but the walk-up entries already work on any platform |
| `packages/core/src/global.ts` | None, informational | State root is `home()/.local/share/opencode`-shaped via `XDG_*`, keyed to the constant app name `"downes"` — deliberate, do not key it off the release channel |

**`random_password()` — `lib.rs:333`.** This function reads 16 bytes from `/dev/urandom` and hex-encodes them into the HTTP Basic credential for the loopback sidecar (`OPENCODE_SERVER_PASSWORD`, set at `lib.rs:692`). If the file open fails, the buffer is left as its initial zeroed state and used anyway. On Windows that open always fails, so every launch ships the same 32-zero password guarding the local engine's control port. Fix this first, before any other porting work: use a real CSPRNG (Windows offers `BCryptGenRandom`; the `rand` or `getrandom` crate abstracts this), and make failure fatal rather than silent.

**`reclaim_shared_store()` — `lib.rs:506`.** This function moves an older build's database out of the shared opencode folder, but only if no process currently has it open. The liveness check shells out to `/usr/sbin/lsof` and folds any failure — including "no such binary" — into `false`, meaning "not busy". Off macOS the probe always fails, so the function reads a live database as free and may move it out from under a running process. Fail closed instead: treat probe failure as "assume busy," not "assume free."

**`reap()` — `lib.rs:282`.** The comment on this function already explains the shape of the problem: the held child handle is `sandbox-exec`, and the actual engine is a grandchild in its own process group, so a plain `child.kill()` does not reach it. The fix on Unix is a `pkill -P <pid>`, gated `#[cfg(unix)]`. Windows has no equivalent shell call; the correct mechanism is a Job Object created before spawning the child and assigned so that closing the window terminates the whole tree. Without this, every window close on Windows leaks one `opencode serve` listener.

**`open_external` and its neighbours — `lib.rs:833-883`.** Three functions pick a launcher program by platform and hand it to `Command::new`. The macOS and Linux choices (`open`, `xdg-open`) are real executables. The Windows choice, `start`, is not: it is a builtin of `cmd.exe`, so the PATH lookup finds nothing and every link, artifact and reveal-adjacent action fails at spawn. Use `cmd /C start ""` with the URL as a separate argument, or call `ShellExecuteW` directly. The empty string matters — `start` reads a lone quoted first argument as the window title.

## Containment

There is none off macOS, by design. The sandbox profile is macOS Seatbelt (`sandbox-exec`), and it has no equivalent shipped for any other platform.

State this rule plainly in any Windows-facing copy: say "works in one folder," never "sandboxed," until a real containment layer lands and passes an escape test. AppContainer is the Windows candidate, and it is unfunded work. `TODO.md` already lists OS containment beyond macOS as a blocker for multi-harness support; this port inherits that dependency.

## Out of scope

Intel macOS, Linux, notarization, code signing.
