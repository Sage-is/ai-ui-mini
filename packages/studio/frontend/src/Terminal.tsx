import { onCleanup, onMount } from "solid-js"
import * as api from "./api"
import { createEngine, type TerminalEngine } from "./terminal-engine"
import { terminalWriter } from "./terminal-writer"

// Runs the branded opencode TUI (a server PTY) inside a web terminal.
// The emulator is behind terminal-engine.ts, so it is a one-line swap.
export default function Terminal(props: { onActivity?: () => void }) {
  let host!: HTMLDivElement
  let ws: WebSocket | undefined
  let ptyID: string | undefined
  let engine: TerminalEngine | undefined
  let output: ReturnType<typeof terminalWriter> | undefined
  let disposed = false
  let cursor = 0

  const dims = () => ({
    cols: engine && engine.cols > 2 ? engine.cols : 120,
    rows: engine && engine.rows > 2 ? engine.rows : 32,
  })
  const pushSize = () => {
    if (!ptyID) return
    const { cols, rows } = dims()
    api.resizePty(ptyID, cols, rows).catch(() => {})
  }
  const fitNow = () => {
    try {
      engine?.fit()
    } catch {}
  }

  const connect = async () => {
    const ticket = await api.connectToken(ptyID!)
    const url = await api.ptyConnectUrl(ptyID!, ticket, cursor)
    ws = new WebSocket(url)
    ws.binaryType = "arraybuffer"
    ws.onmessage = (m) => {
      // Terminal OUTPUT arrives as string frames; binary frames are control.
      if (typeof m.data === "string") {
        cursor += m.data.length
        output?.push(m.data)
        props.onActivity?.()
        return
      }
      const bytes = new Uint8Array(m.data as ArrayBuffer)
      if (bytes[0] === 0) {
        try {
          cursor = JSON.parse(new TextDecoder().decode(bytes.slice(1))).cursor ?? cursor
        } catch {}
      }
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
    try {
      engine = await createEngine()
    } catch (e) {
      host.textContent = "Could not load the terminal engine: " + String(e)
      return
    }
    if (disposed) return

    engine.open(host)
    output = terminalWriter((data, done) => engine!.write(data, done))
    engine.onData((d) => ws?.readyState === WebSocket.OPEN && ws.send(d))
    engine.onResize(() => pushSize())

    const ro = new ResizeObserver(() => fitNow())
    ro.observe(host)
    onCleanup(() => ro.disconnect())
    if (typeof document !== "undefined" && document.fonts) {
      document.fonts.ready.then(() => fitNow())
    }

    // Let layout settle so fit measures real dimensions (0-size → blank).
    await new Promise((r) => requestAnimationFrame(() => r(null)))
    fitNow()

    try {
      const p = await api.createPty()
      ptyID = p.id
      pushSize()
      await connect()
      pushSize()
      engine.focus()
    } catch (e) {
      host.textContent = "Could not start the Downes terminal: " + String(e)
    }
  })

  onCleanup(() => {
    disposed = true
    ws?.close()
    try {
      engine?.dispose()
    } catch {}
  })

  return <div class="term" ref={host} />
}
