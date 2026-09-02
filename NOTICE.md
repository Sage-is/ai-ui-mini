# Third-Party Notices

This repository is MIT licensed (see LICENSE). The shipped .app bundles third-party components under their own terms.

## Annotation Mono

Copyright (c) 2025, Qwerasd (Qwerasd.org). Licensed under the SIL Open Font License 1.1.

Vendored at `packages/studio/frontend/src/fonts/` and compiled into the studio binary, so every bundle carries it. The licence text lives at `packages/studio/frontend/src/fonts/LICENSE-AnnotationMono.txt` and ships in the app at `Contents/Resources/licenses/`.

The Nerd Font build is used whole rather than subset because the terminal draws powerline and icon glyphs. The "Mono" cut is the fixed-width one.

## opencode

MIT. This repo is a fork of https://github.com/anomalyco/opencode at v1.18.18, rebranded per `docs/fork-brand-surface.md` in the parent project.

**Not built by the OpenCode team, and not affiliated with them in any way.**

## startr.style

`packages/studio/frontend/src/startr.style.css` is a vendored snapshot of https://startr.style/style.css taken 2026-08-24 from an unversioned "latest" build. Vendored because the app CSP blocks external hosts. It carries no licence statement. Maintained by Startr LLC and believed first-party, so exposure is low. A belief is not a licence grant — it should be replaced by an explicit one.

## Bundled runtime dependencies

Compiled into the shipped frontend. Versions per `packages/studio/frontend/package.json`:

- solid-js 1.9.10
- marked 18.0.7
- dompurify 3.3.1
- @xterm/xterm 5.5.0
- @xterm/addon-fit 0.10.0
- ghostty-web (pinned git revision, anomalyco/ghostty-web)
- @tauri-apps/api
- @tauri-apps/plugin-opener

Each declares its licence in its own package metadata. These were **not** individually verified when this file was written — the checkout's module layout put them out of reach. This is an inventory, not a clearance.

Check dompurify first. It is dual Apache-2.0 / MPL-2.0, and Apache-2.0 clause 4(d) propagates notice requirements once a NOTICE file exists.
