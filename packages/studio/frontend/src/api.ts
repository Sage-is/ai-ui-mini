// Talks to the opencode sidecar the Tauri shell spawned. Uses the documented
// V2 HTTP API directly (fetch + SSE) to avoid SDK typing churn.
import { invoke } from "@tauri-apps/api/core"

export type ServerInfo = { url: string; username: string; password: string; studio: string }

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
  return r.status === 204 ? (undefined as T) : r.json()
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

// SSE stream of a session's durable events. EventSource can't set headers,
// so auth rides the ?auth_token query param the server supports.
export async function streamSession(sessionID: string, onEvent: (e: any) => void): Promise<EventSource> {
  const s = await server()
  const token = btoa(`${s.username}:${s.password}`)
  const url = `${s.url}/api/session/${sessionID}/event?auth_token=${encodeURIComponent(token)}`
  const es = new EventSource(url)
  es.onmessage = (m) => {
    try {
      onEvent(JSON.parse(m.data))
    } catch {}
  }
  return es
}
