import { onCleanup, onMount } from "solid-js"
import type { Ghostty } from "ghostty-web"
import * as api from "./api"

// ghostty-web is a full terminal emulator (Ghostty in WASM). opencode's TUI
// (opentui) probes advanced capabilities — text-sizing, pixel size, cursor
// readback — that xterm.js does not answer; ghostty does, so the TUI renders.
let shared: Promise<{ mod: typeof import("ghostty-web"); ghostty: Ghostty }> | undefined
const loadGhostty = () => {
  if (!shared) shared = import("ghostty-web").then(async (mod) => ({ mod, ghostty: await mod.Ghostty.load() }))
  return shared
}

export default function Terminal(props: { onActivity?: () => void }) {
  let host!: HTMLDivElement
  let ws: WebSocket | undefined
  let ptyID: string | undefined
  let disposed = false
  let cursor = 0
  let term: any
  let fit: any

  const dims = () => ({
    cols: term?.cols && term.cols > 2 ? term.cols : 120,
    rows: term?.rows && term.rows > 2 ? term.rows : 32,
  })
  const pushSize = () => {
    if (!ptyID) return
    const { cols, rows } = dims()
    api.resizePty(ptyID, cols, rows).catch(() => {})
  }
  const fitNow = () => {
    try {
      fit?.fit()
    } catch {}
  }

  const connect = async () => {
    const ticket = await api.connectToken(ptyID!)
    const url = await api.ptyConnectUrl(ptyID!, ticket, cursor)
    ws = new WebSocket(url)
    ws.binaryType = "arraybuffer"
    ws.onmessage = (m) => {
      const bytes = new Uint8Array(m.data as ArrayBuffer)
      if (bytes[0] === 0) {
        try {
          cursor = JSON.parse(new TextDecoder().decode(bytes.slice(1))).cursor ?? cursor
        } catch {}
        return
      }
      cursor += bytes.length
      term?.write(bytes)
      props.onActivity?.()
    }
    ws.onclose = () => {
      if (!disposed) setTimeout(reconnect, 1000)
    }
  }

  const reconnect = async () => {
    if (disposed) return
    try {
      await connect()
    } catch {
      setTimeout(reconnect, 1500)
    }
  }

  onMount(async () => {
    let mod: typeof import("ghostty-web"), ghostty: Ghostty
    try {
      ;({ mod, ghostty } = await loadGhostty())
    } catch (e) {
      host.textContent = "Could not load the terminal engine: " + String(e)
      return
    }
    if (disposed) return

    term = new mod.Terminal({
      cursorBlink: true,
      cursorStyle: "bar",
      fontSize: 13,
      fontFamily: '"SF Mono","Cascadia Code",Menlo,monospace',
      convertEol: false,
      theme: { background: "#141915", foreground: "#e8ece7", cursor: "#7fb694" },
      scrollback: 10000,
      ghostty,
    })
    fit = new mod.FitAddon()
    term.loadAddon(fit)
    term.open(host)
    term.onData((d: string) => ws?.readyState === WebSocket.OPEN && ws.send(d))
    term.onResize(() => pushSize())
    const ro = new ResizeObserver(() => fitNow())
    ro.observe(host)
    onCleanup(() => ro.disconnect())

    await new Promise((r) => requestAnimationFrame(() => r(null)))
    fitNow()

    try {
      const p = await api.createPty()
      ptyID = p.id
      pushSize()
      await connect()
      pushSize()
      term.focus?.()
    } catch (e) {
      host.textContent = "Could not start the Downes terminal: " + String(e)
    }
  })

  onCleanup(() => {
    disposed = true
    ws?.close()
    try {
      term?.dispose()
    } catch {}
  })

  return <div class="term" ref={host} />
}
