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
