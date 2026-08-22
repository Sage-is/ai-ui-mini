import { createSignal, onMount, onCleanup, For, Show, Switch, Match } from "solid-js"
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import * as api from "./api"
import { md, slides, isDeck, isHtml, htmlDoc, plain } from "./render"
import Terminal from "./Terminal"

type Node = { name: string; path: string; dir: boolean }

// Links in the viewer open in the user's default browser, never the webview.
function externalLinks(e: MouseEvent) {
  const a = (e.target as HTMLElement)?.closest?.("a") as HTMLAnchorElement | null
  if (a?.href && /^https?:\/\//i.test(a.href)) {
    e.preventDefault()
    invoke("open_external", { url: a.href }).catch(() => {})
  }
}

export default function App() {
  const [status, setStatus] = createSignal("starting sidecar…")
  const [ready, setReady] = createSignal(false)
  const [tree, setTree] = createSignal<Node[]>([])
  const [expanded, setExpanded] = createSignal<Record<string, Node[]>>({})
  const [selected, setSelected] = createSignal<string>()
  const [content, setContent] = createSignal("")

  const refreshRoot = async () => {
    try {
      setTree(await invoke<Node[]>("list_dir", { rel: "" }))
    } catch {}
    // refresh any open dirs too, so new artifacts appear live (no flicker)
    for (const key of Object.keys(expanded())) {
      try {
        const kids = await invoke<Node[]>("list_dir", { rel: key })
        setExpanded((cur) => ({ ...cur, [key]: kids }))
      } catch {}
    }
  }

  // Snappy refresh on TUI activity; the interval below is the guarantee.
  let refreshTimer: number | undefined
  const scheduleRefresh = () => {
    clearTimeout(refreshTimer)
    refreshTimer = window.setTimeout(refreshRoot, 600)
  }

  onMount(async () => {
    const ok = await api.waitReady()
    if (!ok) return setStatus("sidecar unreachable")
    setReady(true)
    setStatus("ready · Downes")
    refreshRoot()
    // Poll so the browser always reflects what the TUI writes, activity or not.
    const iv = window.setInterval(refreshRoot, 2000)
    onCleanup(() => clearInterval(iv))

    // Zoom: Cmd/Ctrl +/-/0. Native webview zoom reflows the layout, so the
    // terminal refits. (zoomHotkeysEnabled alone doesn't bind on macOS here.)
    let zoom = 1
    const wv = getCurrentWebview()
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return
      if (e.key === "=" || e.key === "+") zoom = Math.min(zoom + 0.1, 3)
      else if (e.key === "-") zoom = Math.max(zoom - 0.1, 0.5)
      else if (e.key === "0") zoom = 1
      else return
      e.preventDefault()
      wv.setZoom(zoom).catch(() => {})
    }
    window.addEventListener("keydown", onKey)
    onCleanup(() => window.removeEventListener("keydown", onKey))

    // Links clicked inside a sandboxed HTML artifact reach us by postMessage
    // (the frame's only channel out) — route them to the default browser.
    const onMsg = (e: MessageEvent) => {
      const url = (e.data as { __downes_open?: string })?.__downes_open
      if (url && /^https?:\/\//i.test(url)) invoke("open_external", { url }).catch(() => {})
    }
    window.addEventListener("message", onMsg)
    onCleanup(() => window.removeEventListener("message", onMsg))

    // Esc leaves the fullscreen HTML overlay.
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape" && fs()) setFs(false)
    }
    window.addEventListener("keydown", onEsc)
    onCleanup(() => window.removeEventListener("keydown", onEsc))
  })

  const toggleDir = async (n: Node) => {
    const ex = expanded()
    if (ex[n.path]) {
      const { [n.path]: _, ...rest } = ex
      setExpanded(rest)
      return
    }
    try {
      setExpanded({ ...ex, [n.path]: await invoke<Node[]>("list_dir", { rel: n.path }) })
    } catch {}
  }

  const openFile = async (n: Node) => {
    setSelected(n.path)
    try {
      setContent(await invoke<string>("read_file", { rel: n.path }))
    } catch {
      setContent("*Could not read file.*")
    }
  }

  // ---- viewer toolbar --------------------------------------------------
  const showingHtml = () => !!(selected() && isHtml(selected()!))

  // HTML: fullscreen overlay (Esc exits) + open the on-disk file in browser.
  const [fs, setFs] = createSignal(false)
  const openBrowser = () => {
    if (selected()) invoke("open_in_browser", { rel: selected() }).catch(() => {})
  }

  // Markdown: print / Save as PDF. WKWebView has no JS window.print(), so we
  // render a print-ready page, hand it to the browser via Rust, and let it
  // auto-open the print dialog on load (Save as PDF lives there too).
  const doPrint = () => {
    const doc =
      `<!doctype html><html><head><meta charset="utf-8"><title>${selected() || "artifact"}</title>` +
      `<style>body{font:15px/1.65 Georgia,serif;color:#111;max-width:46rem;margin:2rem auto;padding:0 1rem}` +
      `h1,h2,h3{font-family:system-ui,sans-serif;line-height:1.25}` +
      `table{border-collapse:collapse;width:100%}th,td{border:1px solid #999;padding:.4rem .6rem;text-align:left}` +
      `code{font-family:ui-monospace,monospace;background:#f0f0f0;padding:.1em .3em;border-radius:3px}` +
      `pre{background:#f0f0f0;padding:.8rem;border-radius:4px;overflow:auto}</style></head>` +
      `<body>${md(content())}` +
      `<script>window.onload=function(){setTimeout(function(){window.print()},300)}<\/script>` +
      `</body></html>`
    invoke("print_html", { html: doc }).catch(() => {})
  }

  // Copy: three modes, one is the default (switchable). Picking a mode copies
  // it and makes it the new default.
  type CopyMode = "rich" | "markdown" | "plain"
  const [copyMode, setCopyMode] = createSignal<CopyMode>("rich")
  const [menuOpen, setMenuOpen] = createSignal(false)
  const [copied, setCopied] = createSignal(false)
  const flashCopied = () => {
    setCopied(true)
    setTimeout(() => setCopied(false), 1200)
  }
  const runCopy = async (mode: CopyMode) => {
    const src = content()
    try {
      if (mode === "rich" && typeof ClipboardItem !== "undefined") {
        await navigator.clipboard.write([
          new ClipboardItem({
            "text/html": new Blob([md(src)], { type: "text/html" }),
            "text/plain": new Blob([plain(src)], { type: "text/plain" }),
          }),
        ])
      } else if (mode === "markdown") {
        await navigator.clipboard.writeText(src)
      } else {
        await navigator.clipboard.writeText(plain(src))
      }
      flashCopied()
    } catch {}
  }
  const pickCopy = (mode: CopyMode) => {
    setCopyMode(mode)
    setMenuOpen(false)
    runCopy(mode)
  }

  const TreeRow = (p: { n: Node }) => (
    <div>
      <div
        class={`node ${p.n.dir ? "dir" : ""} ${selected() === p.n.path ? "active" : ""}`}
        onClick={() => (p.n.dir ? toggleDir(p.n) : openFile(p.n))}
      >
        <span class="ic">{p.n.dir ? (expanded()[p.n.path] ? "▾" : "▸") : "·"}</span>
        <span>{p.n.name}</span>
      </div>
      <Show when={expanded()[p.n.path]}>
        <div class="indent">
          <For each={expanded()[p.n.path]}>{(c) => <TreeRow n={c} />}</For>
        </div>
      </Show>
    </div>
  )

  // Draggable pane widths: left + right are px, center flexes between them.
  const [leftW, setLeftW] = createSignal(240)
  const [rightW, setRightW] = createSignal(window.innerWidth * 0.34)
  const drag = (which: "left" | "right") => (e: PointerEvent) => {
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    const startX = e.clientX
    const startL = leftW()
    const startR = rightW()
    const move = (ev: PointerEvent) => {
      if (which === "left") {
        setLeftW(Math.min(Math.max(startL + (ev.clientX - startX), 140), window.innerWidth - 400))
      } else {
        setRightW(Math.min(Math.max(startR - (ev.clientX - startX), 220), window.innerWidth - 400))
      }
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  return (
    <div class="app" style={{ "grid-template-columns": `${leftW()}px 6px 1fr 6px ${rightW()}px` }}>
      <div class="topbar">
        <span class="brand">SAGE.IS<small style="margin-left: 0">mini</small></span>
        <span class={`status ${ready() ? "ok" : ""}`}>{status()}</span>
      </div>

      <div class="pane files">
        <h2>Studio</h2>
        <Show when={tree().length} fallback={<div class="empty">Empty studio.</div>}>
          <For each={tree()}>{(n) => <TreeRow n={n} />}</For>
        </Show>
      </div>

      <div class="handle" onPointerDown={drag("left")} title="Drag to resize" />

      <div class="chat">
        <Show when={ready()} fallback={<div class="empty">{status()}</div>}>
          <Terminal onActivity={scheduleRefresh} />
        </Show>
      </div>

      <div class="handle" onPointerDown={drag("right")} title="Drag to resize" />

      <div class="pane viewer" classList={{ flush: showingHtml() }} onClick={externalLinks}>
        <Show when={content()}>
          <div class="viewer-tools">
            <Switch>
              <Match when={showingHtml()}>
                <button class="vtool" title="Fullscreen (Esc to exit)" onClick={() => setFs(true)}>⛶</button>
                <button class="vtool" title="Open in browser" onClick={openBrowser}>↗</button>
              </Match>
              <Match when={!showingHtml()}>
                <button class="vtool" title="Print / Save as PDF" onClick={doPrint}>⎙</button>
                <span class="copywrap">
                  <button class="vtool" title={`Copy (${copyMode()})`} onClick={() => runCopy(copyMode())}>
                    {copied() ? "✓" : "⧉"}
                  </button>
                  <button class="vtool caret" title="Copy as…" onClick={() => setMenuOpen((v) => !v)}>▾</button>
                  <Show when={menuOpen()}>
                    <div class="copymenu">
                      <div classList={{ on: copyMode() === "rich" }} onClick={() => pickCopy("rich")}>Rich text</div>
                      <div classList={{ on: copyMode() === "markdown" }} onClick={() => pickCopy("markdown")}>Markdown</div>
                      <div classList={{ on: copyMode() === "plain" }} onClick={() => pickCopy("plain")}>Plain text</div>
                    </div>
                  </Show>
                </span>
              </Match>
            </Switch>
          </div>
        </Show>
        <Show when={content()} fallback={<div class="empty">Select a file to preview.</div>}>
          <Switch fallback={<div class="md" innerHTML={md(content())} />}>
            <Match when={selected() && isHtml(selected()!)}>
              {/* Sandboxed, no same-origin: the artifact can run its own inline
                  scripts/styles but cannot reach the shell or the sidecar. */}
              <iframe class="htmlframe" title="HTML artifact" sandbox="allow-scripts" srcdoc={htmlDoc(content())} />
            </Match>
            <Match when={selected() && isDeck(selected()!, content())}>
              <div class="deck">
                <For each={slides(content())}>
                  {(s) => (
                    <div class="slide">
                      <div innerHTML={s.body} />
                      <Show when={s.notes}><div class="notes">{s.notes}</div></Show>
                    </div>
                  )}
                </For>
              </div>
            </Match>
          </Switch>
        </Show>
        <Show when={fs() && showingHtml()}>
          <div class="fs-overlay">
            <button class="fs-close" title="Exit fullscreen (Esc)" onClick={() => setFs(false)}>✕</button>
            <iframe class="fs-frame" title="HTML artifact" sandbox="allow-scripts" srcdoc={htmlDoc(content())} />
          </div>
        </Show>
      </div>
    </div>
  )
}
