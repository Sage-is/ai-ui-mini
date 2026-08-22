import { onCleanup, onMount } from "solid-js"
import { Terminal as Xterm } from "@xterm/xterm"
import { FitAddon } from "@xterm/addon-fit"
import "@xterm/xterm/css/xterm.css"
import * as api from "./api"

// Embeds the real opencode TUI: a server PTY runs `opencode` in the studio,
// its raw byte stream renders in xterm. Nothing about the agent is reinvented.
export default function Terminal(props: { onActivity?: () => void }) {
  let host!: HTMLDivElement
  let ws: WebSocket | undefined
  let ptyID: string | undefined
  let disposed = false
  let cursor = 0

  const term = new Xterm({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: '"SF Mono","Cascadia Code",Menlo,monospace',
    theme: {
      background: "#141915",
      foreground: "#e8ece7",
      cursor: "#7fb694",
      selectionBackground: "#223027",
    },
    scrollback: 10000,
  })
  const fit = new FitAddon()
  term.loadAddon(fit)

  const fitNow = () => {
    try {
      fit.fit()
    } catch {}
  }
  const dims = () => ({
    cols: term.cols && term.cols > 2 ? term.cols : 120,
    rows: term.rows && term.rows > 2 ? term.rows : 32,
  })
  const pushSize = () => {
    if (!ptyID) return
    const { cols, rows } = dims()
    api.resizePty(ptyID, cols, rows).catch(() => {})
  }

  const connect = async () => {
    const ticket = await api.connectToken(ptyID!)
    const url = await api.ptyConnectUrl(ptyID!, ticket, cursor)
    ws = new WebSocket(url)
    ws.binaryType = "arraybuffer"
    ws.onmessage = (m) => {
      const bytes = new Uint8Array(m.data as ArrayBuffer)
      if (bytes[0] === 0) {
        // control frame: {"cursor": n}
        try {
          cursor = JSON.parse(new TextDecoder().decode(bytes.slice(1))).cursor ?? cursor
        } catch {}
        return
      }
      cursor += bytes.length
      term.write(bytes)
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
    term.open(host)
    term.onData((d) => ws?.readyState === WebSocket.OPEN && ws.send(d))
    term.onResize(() => pushSize())
    // Re-fit whenever the pane changes size; each fit pushes the new size.
    const ro = new ResizeObserver(() => fitNow())
    ro.observe(host)
    onCleanup(() => ro.disconnect())

    // Let the grid/flex layout settle so fit measures real dimensions,
    // otherwise the TUI renders into a 0-size buffer (blank + cursor).
    await new Promise((r) => requestAnimationFrame(() => r(null)))
    fitNow()

    try {
      const p = await api.createPty()
      ptyID = p.id
      pushSize() // give the TUI a real size before it draws
      await connect()
      pushSize()
      term.focus()
    } catch (e) {
      term.write(`\r\n\x1b[31mCould not start the Downes terminal:\x1b[0m ${String(e)}\r\n`)
    }
  })

  onCleanup(() => {
    disposed = true
    ws?.close()
    term.dispose()
  })

  return <div class="term" ref={host} />
}
