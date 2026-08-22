// Talks to the opencode sidecar the Tauri shell spawned. Uses the documented
// V2 HTTP API directly (fetch + SSE) to avoid SDK typing churn.
import { invoke } from "@tauri-apps/api/core"

export type ServerInfo = { url: string; username: string; password: string; studio: string; fork: string }

let info: ServerInfo | null = null

export async function server(): Promise<ServerInfo> {
  if (!info) info = await invoke<ServerInfo>("studio_server")
  return info
}

function basic(s: ServerInfo) {
  return "Basic " + btoa(`${s.username}:${s.password}`)
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const s = await server()
  const r = await fetch(s.url + path, {
    ...init,
    headers: {
      "content-type": "application/json",
      authorization: basic(s),
      "x-opencode-directory": s.studio,
      ...(init?.headers || {}),
    },
  })
  if (!r.ok) throw new Error(`${path} → ${r.status}`)
  if (r.status === 204) return undefined as T
  const j = await r.json()
  // V2 API wraps payloads as { location, data }; unwrap to the data.
  return (j && typeof j === "object" && "data" in j && "location" in j ? j.data : j) as T
}

export async function health(): Promise<boolean> {
  try {
    await api("/api/health")
    return true
  } catch {
    return false
  }
}

export async function waitReady(tries = 60): Promise<boolean> {
  for (let i = 0; i < tries; i++) {
    if (await health()) return true
    await new Promise((r) => setTimeout(r, 500))
  }
  return false
}

export async function createSession(agent = "downes") {
  return api<{ id: string }>("/api/session", {
    method: "POST",
    body: JSON.stringify({ agent }),
  })
}

export async function sendPrompt(sessionID: string, prompt: string) {
  return api(`/api/session/${sessionID}/prompt`, {
    method: "POST",
    body: JSON.stringify({ prompt }),
  })
}

// ---- PTY: run the real opencode TUI in a terminal, stream to the webview ----

// Create a PTY that runs the BRANDED fork TUI (from source) in the studio.
// Running `opencode` off PATH would launch the stock, unbranded binary; the
// fork's index.ts carries the sage.is wordmark and downes agent. The studio
// path as the positional project makes the TUI operate there.
export async function createPty(): Promise<{ id: string }> {
  const s = await server()
  return api<{ id: string }>(
    `/api/pty?location[directory]=${encodeURIComponent(s.studio)}`,
    {
      method: "POST",
      body: JSON.stringify({
        command: "bun",
        args: ["run", "--conditions=browser", "--cwd", s.fork, "src/index.ts", s.studio],
        cwd: s.studio,
      }),
    },
  )
}

// Single-use, ~60s ticket that lets the WebSocket upgrade skip Basic auth.
export async function connectToken(ptyID: string): Promise<string> {
  const s = await server()
  const r = await api<{ ticket: string }>(
    `/api/pty/${ptyID}/connect-token?location[directory]=${encodeURIComponent(s.studio)}`,
    { method: "POST", headers: { "x-opencode-ticket": "1" } },
  )
  return r.ticket
}

export async function ptyConnectUrl(ptyID: string, ticket: string, cursor = 0): Promise<string> {
  const s = await server()
  const ws = s.url.replace(/^http/, "ws")
  const dir = encodeURIComponent(s.studio)
  return `${ws}/api/pty/${ptyID}/connect?location[directory]=${dir}&cursor=${cursor}&ticket=${ticket}`
}

// Out-of-band resize (not sent over the socket).
export async function resizePty(ptyID: string, cols: number, rows: number) {
  const s = await server()
  return api(`/api/pty/${ptyID}?location[directory]=${encodeURIComponent(s.studio)}`, {
    method: "PUT",
    body: JSON.stringify({ size: { cols, rows } }),
  })
}
