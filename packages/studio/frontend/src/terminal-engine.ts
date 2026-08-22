// Swappable terminal engine. xterm.js and ghostty-web share the same surface,
// so the rest of the app talks to this interface and never a concrete engine.
// Switch engines by changing ENGINE (or VITE_TERM_ENGINE). Each engine is a
// dynamic import, so only the selected one enters the bundle.

export type EngineKind = "xterm" | "ghostty" | "wterm"

export const ENGINE: EngineKind =
  (import.meta.env?.VITE_TERM_ENGINE as EngineKind) || "xterm"

export interface TerminalEngine {
  open(el: HTMLElement): void
  write(data: string, done?: () => void): void
  onData(cb: (data: string) => void): void
  onResize(cb: (size: { cols: number; rows: number }) => void): void
  fit(): void
  readonly cols: number
  readonly rows: number
  focus(): void
  dispose(): void
}

const THEME = { background: "#141915", foreground: "#e8ece7", cursor: "#7fb694" }
const FONT = '"SF Mono","Cascadia Code",Menlo,monospace'

async function xtermEngine(): Promise<TerminalEngine> {
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import("@xterm/xterm"),
    import("@xterm/addon-fit"),
  ])
  const term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: FONT,
    theme: THEME,
    scrollback: 10000,
    allowProposedApi: true,
  })
  const fit = new FitAddon()
  term.loadAddon(fit)
  return {
    open: (el) => term.open(el),
    write: (d, done) => term.write(d, done),
    onData: (cb) => term.onData(cb),
    onResize: (cb) => term.onResize(cb),
    fit: () => fit.fit(),
    get cols() {
      return term.cols
    },
    get rows() {
      return term.rows
    },
    focus: () => term.focus(),
    dispose: () => term.dispose(),
  }
}

async function ghosttyEngine(): Promise<TerminalEngine> {
  const mod = await import("ghostty-web")
  const ghostty = await mod.Ghostty.load()
  const term: any = new mod.Terminal({
    cursorBlink: true,
    cursorStyle: "bar",
    fontSize: 13,
    fontFamily: FONT,
    convertEol: false,
    allowTransparency: false,
    theme: THEME,
    scrollback: 10000,
    ghostty,
  })
  const fit: any = new mod.FitAddon()
  term.loadAddon(fit)
  return {
    open: (el) => term.open(el),
    write: (d, done) => term.write(d, done),
    onData: (cb) => term.onData(cb),
    onResize: (cb) => term.onResize(cb),
    fit: () => fit.fit(),
    get cols() {
      return term.cols
    },
    get rows() {
      return term.rows
    },
    focus: () => term.focus?.(),
    dispose: () => term.dispose(),
  }
}

export async function createEngine(kind: EngineKind = ENGINE): Promise<TerminalEngine> {
  switch (kind) {
    case "ghostty":
      return ghosttyEngine()
    case "wterm":
      throw new Error("wterm engine not implemented yet")
    case "xterm":
    default:
      return xtermEngine()
  }
}
