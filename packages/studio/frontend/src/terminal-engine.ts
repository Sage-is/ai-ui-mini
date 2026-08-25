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
  setTheme(theme: TerminalTheme): void
  dispose(): void
}

export type TerminalTheme = { background: string; foreground: string; cursor: string }

// The emulator needs concrete colors, not CSS variables, so read the resolved
// tokens off <html> and hand them over. Called again on every theme flip so
// the terminal pane never sits at odds with the chrome around it.
export function themeFromCss(): TerminalTheme {
  const s = getComputedStyle(document.documentElement)
  const pick = (name: string, fallback: string) => s.getPropertyValue(name).trim() || fallback
  return {
    background: pick("--background", "#141915"),
    foreground: pick("--text-main", "#e8ece7"),
    cursor: pick("--accent", "#7fb694"),
  }
}

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
    theme: themeFromCss(),
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
    setTheme: (t) => {
      term.options.theme = t
    },
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
    theme: themeFromCss(),
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
    setTheme: (t) => {
      term.options = { ...term.options, theme: t }
    },
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
