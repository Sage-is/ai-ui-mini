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
- [x] Drag files and folders INTO the sidebar — onto a folder row or the pane
  for the studio root. Skips dotfiles and symlinks, never overwrites; 7 tests
  cover the rules
- [x] Reveal in Finder on every sidebar row, files and folders alike — the
  drag-out substitute, and the only way to export a whole course folder
- [x] Status line names the running product, so mini stops calling itself
  Downes
- [x] Own state root per product — `XDG_*` points at `$STUDIO/.downes/xdg`, so
  mini, Downes and a stock opencode stop sharing one `auth.json` and database.
  Seeded from the user's real store on first run, so isolation costs no second
  login; `DOWNES_SHARE_STATE=1` opts out
- [x] Layer-3 sandbox actually wired — on BOTH surfaces: the terminal launcher
  and the studio's sidecar spawn. Profile ships in the payload; paths are
  resolved with `pwd -P` because the sandbox canonicalizes before matching, so
  an unresolved `/var/folders/...` made the TMP rule silently dead. Escape test
  is 22 cases, runs the shipped launcher rather than its own env, distinguishes
  "denied" from "target absent", and covers `serve` — `--version` never binds a
  port, which is how a missing `network-bind` rule went unnoticed
- [x] Read fence widened past the original four paths — cloud, forge and agent
  credential stores, browser profiles, iCloud Drive, dotfile secrets. Still a
  deny-list over a blanket read-allow, and the profile says so
- [x] The app lands in `~/Applications` — the launcher links it, because
  Homebrew `post_install` provably cannot

## In Progress / TODO

- [ ] **Verify the brew install on a second Mac** #task — installed and
  released, not yet confirmed by anyone but the build machine
  - [ ] `brew install sage-is/apps/mini` on a clean account; no Gatekeeper warning
  - [ ] engine starts with bun, node and opencode absent from PATH

## Backlog

- [ ] **Drag files OUT of the sidebar** — WKWebView cannot start a native file
  drag from a web page. Reveal in Finder ships instead (see Delivered)
  - [ ] spike `tauri-plugin-drag` (NSDraggingSession): licence, and whether it
    fights `dragDropEnabled`, which drop-in depends on

- [ ] **A dropped file is untrusted content the agent then reads.** Drop-in is
  the first path by which content the teacher did not author enters the studio.
  The prompt-injection card from the panel review stops being hypothetical
  - [ ] the import fence is filesystem-level only; nothing inspects content
  - [ ] `AGENTS.md` in the studio is attached as instructions with no flag to
    stop it (`session/instruction.ts:64`) — a downloaded course shipping one
    gets its text treated as instruction. `CLAUDE.md` is the same vector and
    IS now closed by `OPENCODE_DISABLE_CLAUDE_CODE=1`; `AGENTS.md` is not

- [ ] **Multi-harness support** — run pi and deepseek-harness alongside
  opencode
  - [ ] harness switcher UI
  - [ ] per-harness config
  - [ ] research spike on each project's runtime requirements and licence
    (unknown; licence matters because mini is MIT and bundling changes that)

- [ ] **Reads are a deny-list, not a fence** — `downes.sb` allows `file-read*`
  broadly and denies known-secret paths back, so anything not on the list stays
  readable, and `:443` egress makes it exfiltratable
  - [ ] flip reads to deny-default with an allowlist; needs the dyld shared
    cache, frameworks and dylibs enumerated first
  - [ ] `/private/tmp` is writable because bash heredocs need it — revisit if
    reads ever become deny-default

- [ ] **OS-level containment beyond macOS** — BLOCKS multi-harness. macOS is
  done (see Delivered); Linux and Windows are unclaimed and the launcher
  leaves one seam for a backend
  - [ ] Linux: Landlock, not bubblewrap — ladder in
    `docs/decisions/vm-containment.md`
  - [ ] Windows: still deferred, `docs/decisions/windows-sandbox.md`
  - [ ] `sandbox-exec` is deprecated by Apple with no removal date; App
    Sandbox needs codesigning we do not have yet

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
