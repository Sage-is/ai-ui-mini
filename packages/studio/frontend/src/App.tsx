import { createSignal, onMount, onCleanup, For, Show } from "solid-js"
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import * as api from "./api"
import { md, slides, isDeck } from "./render"
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

      <div class="pane viewer" onClick={externalLinks}>
        <Show when={content()} fallback={<div class="empty">Select a file to preview.</div>}>
          <Show
            when={selected() && isDeck(selected()!, content())}
            fallback={<div class="md" innerHTML={md(content())} />}
          >
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
          </Show>
        </Show>
      </div>
    </div>
  )
}
