# Downes studio — Sage.is AI-UI mini shell

A lightweight **Tauri v2** desktop shell that wraps the branded opencode TUI
in a three-panel curriculum studio for teachers. Nothing about the agent is
reinvented — the studio hosts opencode; it does not replace it.

## Layout

- **Left — file manager.** A Rust-fenced tree over the studio directory
  (`~/Downes`). Dotfiles and studio plumbing hidden. Polls every 2s so
  files appear live as the TUI writes them.
- **Center — the TUI.** The compiled fork binary runs as a server **PTY**;
  its byte stream renders in an `xterm.js` engine over a ticket-gated
  WebSocket. This is opencode's own terminal machinery, reused whole.
- **Right — artifact viewer.** Markdown via `marked`+`dompurify`; reveal.js
  decks split on `---`; `http(s)` links open in the default browser.

## Architecture

```
Tauri window (Rust, src-tauri/)
  ├─ spawns:   opencode `serve`  (loopback sidecar, Basic-auth, studio-scoped)
  ├─ injects:  {url, user, password, studio, fork, bin} -> webview
  ├─ commands: list_dir, read_file (studio-fenced), open_external
  └─ loads:    frontend/ (Solid + Vite)
                 └─ createPty {command: <compiled bin>, cwd: studio}
                    -> ticket -> WebSocket -> terminal-engine -> xterm
```

Key files:
- `src-tauri/src/lib.rs` — sidecar spawn, credential injection, fenced fs
  commands, `open_external`, compiled-binary resolution.
- `frontend/src/api.ts` — the V2 HTTP/PTY surface (unwraps the `{location,
  data}` envelope; PTY create/ticket/connect/resize).
- `frontend/src/terminal-engine.ts` — **swappable** emulator adapter
  (`xterm` now; `ghostty`, `wterm` lazy). Switch via `VITE_TERM_ENGINE`.
- `frontend/src/Terminal.tsx` — WS wiring: **string frames = TUI output**,
  binary `0x00` frames = cursor control.
- `frontend/src/App.tsx` — panes, drag-resize, zoom (Cmd +/-/0), live poll.

## Run (dev)

```bash
bun install                              # in the fork root (ai-ui-mini)
bun run --cwd packages/opencode build    # compile the TUI binary (once)
cd packages/studio && bunx tauri dev
```

`DOWNES_STUDIO=<dir>` overrides the studio (default `~/Downes`).

## Build (Windows)

Needs the MSVC Rust toolchain (`winget install Rustlang.Rustup`), VS C++
build tools, and the WebView2 runtime.

```bash
# The engine, from the fork root. Both variables matter: unset, the build
# stamps the version 0.0.0-<branch>-<timestamp>, which fails provider gates
# asking for a minimum opencode version, and names the database after the
# current git branch, so a Windows install disagrees with a macOS one.
OPENCODE_CHANNEL=downes/v1 \
OPENCODE_VERSION=$(bun packages/studio/scripts/engine-version.ts) \
  bun packages/opencode/script/build.ts --single
cd packages/studio && bunx tauri build --config src-tauri/tauri.mini.conf.json
```

In PowerShell, set them first instead: `$env:OPENCODE_CHANNEL = "downes/v1"`
and `$env:OPENCODE_VERSION = (bun packages/studio/scripts/engine-version.ts)`.

Windows ships **mini**, so pass the mini config — the staged product marker
says `SAGE.ISmini`, and a plain `tauri build` would name the app Downes while
that marker sent it to `~/SAGE.ISmini`. To build Downes here instead, override
both: `DOWNES_PRODUCT=Downes bunx tauri build`.

`tauri.windows.conf.json` is merged automatically on Windows, with or without
`--config`. It sets the bundle target to `nsis` — the base config's `app`/`dmg`
are macOS-only — and stages the payload into the bundle:

- `beforeBuildCommand` runs `scripts/stage-payload.ts`, which copies the
  host-arch engine to `src-tauri/payload/bin/opencode.exe` and writes the
  product marker (both gitignored).
- `bundle.resources` maps those to `bin/opencode.exe` and `product`, so NSIS
  installs them beside the app exe, where `engine_bin()` and
  `product_workspace()` look first.

The marker is not optional on Windows. Unmarked, `product_workspace()` falls
back to `bundle_identifier()`, which parses `Info.plist` — a file no Windows
bundle has — so mini would name its window correctly and still write into
`~/Downes`.

### Installer branding

`scripts/make-installer-art.py` draws `installer-sidebar.bmp` (the Welcome and
Finish panel) and `installer-header.bmp` (the band on the other pages) from
`app-icon-mini.png`, in sage.education's palette. It is an offline tool, not
part of the build — run it and commit the bitmaps when the icon changes.

Both are drawn at 2x the classic control sizes and paired with
`MUI_..._BITMAP_STRETCH "AspectFitHeight"` in `installer.nsi`. MUI's default is
`FitControl`, which stretches to fill; the installer is DPI-aware and dialog
units do not scale equally on both axes, so on a scaled display that default
squashes the artwork by about 5% and softens the text by upscaling.

`installerIcon`/`uninstallerIcon` must be set explicitly — Tauri does NOT fall
back to `bundle.icon` for them, and unset means NSIS's stock icon ships on the
setup .exe.

Like the product marker, these assets are mini's. A Downes-branded Windows
build would carry them.

That staging is what macOS gets from the Homebrew cask, and it is why an
installed copy works at all: engine resolution is relative to `current_exe()`,
so a bundle with no engine and no fork above it has nothing to run. Cross-arch
bundles need the engine built for the target being bundled, not the host.

The installer lands in `src-tauri/target/release/bundle/nsis/`.

## Why the compiled binary

Running the TUI from source via `bun run src/index.ts` keeps bun's runtime
hot (~34% idle CPU + a busy sidecar). Running the compiled binary
(`dist/opencode-darwin-<arch>/bin/opencode`) drops idle to ~15% and quiets
the sidecar. Rust prefers the binary and falls back to source; the launcher
does the same. The binary is a gitignored build artifact.

## Design decisions worth knowing

- **The V2 API wraps every payload as `{location, data}`** — always unwrap.
- **Terminal output is string frames, not binary** — the blank-terminal
  bug was casting strings to `Uint8Array`.
- **opentui needs no special terminal** — its capability probes are
  fire-and-forget with fallbacks, so xterm.js renders it (set
  `COLORTERM=truecolor`).
- **Zoom** uses the native webview `setZoom` (reflows, so the terminal
  refits); `zoomHotkeysEnabled` alone did not bind on this macOS webview.
- **Links** open via a Rust `open_external` command, not the opener
  plugin's scope config.
