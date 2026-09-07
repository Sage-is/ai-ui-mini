# SAGE.IS mini

SAGE.IS mini is a self-contained macOS desktop studio: a Tauri shell around a bundled AI coding engine. mini is the platform, not an agent — no agent ships inside it. Downes, the curriculum agent, is a separate product built on mini, in the AGPL repo `Sage-is/AI-Education-Downes`.

## Install

```bash
brew tap sage-is/apps
brew install --cask sage-is/apps/mini
```

Apple Silicon only. The workspace lands at `~/SAGE.ISmini`.

The app is ad-hoc signed, not notarized. The cask clears quarantine on install. Notarization is pending.

## Platform support

macOS on Apple Silicon is the only build that ships. There is no Intel build; that needs an x86_64 runner.

Windows and Linux are unclaimed, not refused. The Tauri shell is already written for them. `packages/studio/src-tauri/src/lib.rs` branches per platform for the engine name (`opencode.exe`), for Reveal (`explorer /select,` on Windows, `xdg-open` on Linux) and for opening links. What is missing is the build and the delivery: the packaging script and the Homebrew cask are macOS-only, `tauri.conf.json` targets `app` and `dmg`, and the engine binary must be compiled per target.

Containment does not port, by design. `sandbox_prefix()` returns `None` off macOS, because the profile is macOS Seatbelt and has no equivalent we ship elsewhere. A Windows or Linux build therefore runs with no OS-level fence. Its honest claim is "works in one folder" — never "sandboxed". That wording is a rule, not a preference, and it holds until a real containment layer lands and passes an escape test. Linux's candidate is Landlock; Windows' is AppContainer, and both are unfunded work.

## What it does

The studio window has three panes:

- **Left — file manager.** A Rust-fenced tree over the studio directory. Dotfiles and studio plumbing stay hidden. It polls for live updates.
- **Center — the TUI.** The compiled engine binary runs as a server PTY, rendered through an xterm.js terminal over a ticket-gated WebSocket.
- **Right — artifact viewer.** Renders Markdown and slide decks; links open in the default browser.

Drag files and folders into the sidebar to add them to the workspace. Every sidebar row supports Reveal in Finder, files and folders alike — the way to get content back out. The interface has a light/dark toggle.

## Isolation

mini keeps its own state root, separate from any stock opencode install or from Downes: `XDG_*` points inside the studio directory, so auth tokens and databases don't collide across products. State is seeded from the user's existing store on first run, so isolation costs no second login.

A sandbox profile (`sandbox-exec`, macOS only) fences the process at launch, on both the terminal launcher and the studio's sidecar spawn. File reads are a deny-list, not a fence: broad read access is allowed, with known-secret paths denied back. Network egress is TLS-only; hostnames are not pinned. Treat the sandbox as a hardening layer, not a full containment guarantee.

## Development

```bash
bun install
bun run --cwd packages/opencode build
cd packages/studio && bunx tauri dev
```

## Licence and provenance

MIT. This repo is a fork of https://github.com/anomalyco/opencode at v1.18.18.

Not built by the OpenCode team, and not affiliated with them in any way.

Third-party components and their terms are listed in `NOTICE.md`.
