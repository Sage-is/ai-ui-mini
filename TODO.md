# Sage.is AI-UI mini — Chart

> Navigate chart, top → down.
> mini is the platform; Downes is the first agent shipped on it.
> Ships via Homebrew as `brew install sage-is/apps/mini`.

## Destination

mini is the platform; Downes is the first agent shipped on it. Ships via
Homebrew as `brew install sage-is/apps/mini`.

## Notes

This repo is the MIT platform. The Downes curriculum agent lives in
`Sage-is/AI-Education-Downes` (AGPL) and tracks this one as a submodule.
Payloads are built by `scripts/package_macos.sh` there, which emits both
products from one Rust binary; they differ by bundle metadata and a `product`
marker staged in the payload.

Release assets follow the licence: mini ships from this repo, Downes from the
AGPL one, because its payload combines both.

Long-form reasoning behind these cards is internal and not in git — see
`docs/dossiers/` in the AI-Education-Downes checkout. Cards here state the
work; they do not restate the argument.

`.todoscope-exclude.csv` fences the scan to our own packages: this is a fork,
and most of the tree is upstream's.

## Delivered

- [x] Self-contained brew payload — no bun or node needed; engine bundled as
  a single-file executable
- [x] Own identity — `is.sage.mini` bundle ID, plain indigo hex-S icon,
  `tauri.mini.conf.json`
- [x] Install-relative engine resolution (`engine_bin()` walks up from
  `current_exe()`)
- [x] First-run workspace bootstrap in `~/SageMini`
- [x] startr.style theming with light/dark toggle

## In Progress / TODO

- [ ] **Verify the brew install on a second Mac** #task — installed and
  released, not yet confirmed by anyone but the build machine
  - [ ] `brew install sage-is/apps/mini` on a clean account; no Gatekeeper warning
  - [ ] engine starts with bun, node and opencode absent from PATH

## Backlog

- [ ] **Multi-harness support** — run pi and deepseek-harness alongside
  opencode
  - [ ] harness switcher UI
  - [ ] per-harness config
  - [ ] research spike on each project's runtime requirements and licence
    (unknown; licence matters because mini is MIT and bundling changes that)

- [ ] **OS-level containment** — BLOCKS multi-harness
  - [ ] wire sandbox-exec into `launcher/downes.sh` (header advertises sandbox
    prefix but contains no sandbox-exec call)
  - [ ] run the escape test in CI (`test/sandbox/escape-test.sh` is currently
    orphaned from any Make target or workflow)
  - [ ] correct any board or copy that claims sandboxing before it is true

- [ ] **Folder guarantee enforcement** — 'works in one folder' currently comes
  from opencode's permission config, not the OS. A third-party TUI will not
  honour it, so multi-harness makes OS containment mandatory rather than
  optional.

- [ ] **Per-harness brew formulas** (mini-pi, mini-deepseek) so users pay only
  for what they use, rather than bundling three ~136 MB engines or fetching
  unsigned code at runtime
  - [ ] risk: the 'a Mac with only Homebrew' promise breaks if a harness
    needs node, python, or uv

- [ ] **Credentials and egress multiply** — three vendors' auth stores in one
  process tree; SBPL cannot pin hostnames so the honest claim stays
  'TLS-only egress'

- [ ] **Intel build** — Apple Silicon only; needs an x86_64 CI runner

- [ ] **Notarization** — blocked on the Startr LLC Apple enrolment; brew
  sidesteps it meanwhile
