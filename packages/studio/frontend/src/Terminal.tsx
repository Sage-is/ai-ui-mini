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

  const pushSize = () => {
    if (ptyID) api.resizePty(ptyID, term.cols, term.rows).catch(() => {})
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
    fit.fit()
    term.onData((d) => ws?.readyState === WebSocket.OPEN && ws.send(d))
    term.onResize(() => pushSize())
    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
      } catch {}
    })
    ro.observe(host)
    onCleanup(() => ro.disconnect())

    try {
      const p = await api.createPty()
      ptyID = p.id
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
