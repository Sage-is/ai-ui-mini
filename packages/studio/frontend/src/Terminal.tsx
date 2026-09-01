import { onCleanup, onMount } from "solid-js"
import * as api from "./api"
import { createEngine, themeFromCss, type TerminalEngine } from "./terminal-engine"
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
    startedAt = Date.now()
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
      if (disposed) return
      // A pane that died seconds after starting is a failing child, not a
      // dropped connection. Only the short-lived case counts as a failure;
      // a terminal the teacher used for an hour resets the count.
      if (Date.now() - startedAt < SHORT_LIFE_MS) failures++
      else failures = 0
      if (failures >= MAX_FAILURES)
        return giveUp("The terminal exited immediately " + MAX_FAILURES + " times running:\n" + lastCmd)
      setTimeout(recover, backoff())
    }
  }

  // A pty that dies immediately used to be respawned forever, roughly once a
  // second, each attempt printing its own error: 22 cycles in 26 seconds on
  // 0.1.6. The loop hid the cause, because nothing ever reported why the child
  // exited. Count consecutive failures, back off, and give up with the reason
  // on screen. A pane that spawns and lives resets the count.
  let failures = 0
  let startedAt = 0
  let lastCmd = "(unknown)"
  const MAX_FAILURES = 5
  const SHORT_LIFE_MS = 5000
  const backoff = () => Math.min(1000 * 2 ** failures, 15000)

  const giveUp = (why: string) => {
    disposed = true
    host.textContent =
      "The Downes terminal stopped after " + MAX_FAILURES + " failed starts.\n\n" + why +
      "\n\nReopen the app to try again."
  }

  // On disconnect: if the pty still lives, reconnect and replay. If the TUI
  // quit or crashed, spawn a fresh one that --continues the prior session.
  const recover = async () => {
    if (disposed) return
    try {
      if (ptyID && (await api.ptyAlive(ptyID))) {
        await connect()
        return
      }
      // Resume only while the session looks healthy. If --continue is what
      // keeps killing the child, retrying it unchanged can never recover.
      const p = await api.createPty(failures === 0)
      ptyID = p.id
      lastCmd = p.cmd
      cursor = 0
      pushSize()
      await connect()
      pushSize()
      // Deliberately NOT resetting `failures` here. Spawning succeeds even
      // when the child dies a moment later, so resetting on a successful
      // spawn made the counter unreachable and the loop ran forever at a flat
      // interval. ws.onclose owns the reset, and only for a pane that lived.
    } catch (e) {
      failures++
      if (failures >= MAX_FAILURES) return giveUp(String(e))
      setTimeout(recover, backoff())
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

    // Repaint the emulator when the app theme flips, so the terminal pane
    // never sits at odds with the chrome. data-theme is set on <html>.
    const themeObs = new MutationObserver(() => engine?.setTheme(themeFromCss()))
    themeObs.observe(document.documentElement, { attributeFilter: ["data-theme"] })
    onCleanup(() => themeObs.disconnect())
    if (typeof document !== "undefined" && document.fonts) {
      document.fonts.ready.then(() => fitNow())
    }

    // Let layout settle so fit measures real dimensions (0-size → blank).
    await new Promise((r) => requestAnimationFrame(() => r(null)))
    fitNow()

    try {
      const p = await api.createPty()
      ptyID = p.id
      lastCmd = p.cmd
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
