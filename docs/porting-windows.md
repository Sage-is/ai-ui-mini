# SAGE.IS mini on Windows

Windows support shipped in 0.1.13 (PR #1, by @josesimas). This page records
what that covers and what it does not. The build is documented once, in
[`packages/studio/README.md`](../packages/studio/README.md); this page does not
repeat it.

Verified by the contributor on Windows 11. Nobody else on the project has run
it on Windows yet.

## What works

- **Installer.** NSIS, with the engine and the product marker staged into the
  bundle (`packages/studio/scripts/stage-payload.ts`,
  `src-tauri/tauri.windows.conf.json`). An installed copy carries its own
  engine; there is no Homebrew equivalent to stage one.
- **Workspace.** Resolved from `USERPROFILE`, made absolute once, so every
  consumer names the same directory.
- **No stray console.** The sidecar spawns with `CREATE_NO_WINDOW`.
- **Links, artifacts and Save as PDF.** Routed through the opener plugin.
  `start` is a `cmd.exe` builtin and could never be spawned as a program.
- **Sidecar password.** From the OS CSPRNG on every platform. The old code read
  `/dev/urandom`, ignored the failure, and on Windows produced 32 zeros.
- **Window geometry.** Size, position and maximised state persist between runs,
  per product, in the app config directory.

## What remains

All line numbers are in `packages/studio/src-tauri/src/lib.rs`.

| Line | Gap | Effect on Windows |
|------|-----|-------------------|
| 337 | `reap()` shells out to `pkill` under `#[cfg(unix)]` | Closing the window leaks the engine process. It holds its port until the user logs out. A Job Object is the Windows equivalent. |
| 551 | `reclaim_shared_store()` probes with `/usr/sbin/lsof` and folds failure into `false` | A live database reads as free to move. The probe should fail closed. Only reached when an older workspace is adopted. |
| 539 | `session_count()` runs `/usr/bin/sqlite3` | Returns `None`, so the adoption prompt cannot say how many sessions it found. Degrades safely. |
| 510 | The seeded `auth.json` gets `0600` under `#[cfg(unix)]` | Credentials inherit directory permissions. Needs an ACL. |
| 406 | `find_bun()` searches Unix paths | Affects the dev fallback only; a packaged build never looks for bun. |

## Containment

There is none on Windows, and the copy must not imply otherwise. The macOS
sandbox is a Seatbelt profile with no Windows counterpart; `sandbox_prefix()`
returns `None` off macOS. Until an AppContainer layer exists and passes an
escape test, Windows copy says the app **works in one folder**, never that it
is sandboxed.

## Out of scope

Intel macOS, Linux, notarisation, and code signing. The installer is unsigned,
so SmartScreen warns on first run: **More info → Run anyway**.
