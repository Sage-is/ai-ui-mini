import { createSignal, onMount, For, Show } from "solid-js"
import { invoke } from "@tauri-apps/api/core"
import * as api from "./api"
import { md, slides, isDeck } from "./render"

type Node = { name: string; path: string; dir: boolean }
type Msg = { role: "user" | "assistant" | "tool"; text: string }

export default function App() {
  const [status, setStatus] = createSignal("starting sidecar…")
  const [ready, setReady] = createSignal(false)
  const [session, setSession] = createSignal<string>()
  const [msgs, setMsgs] = createSignal<Msg[]>([])
  const [busy, setBusy] = createSignal(false)
  const [tree, setTree] = createSignal<Node[]>([])
  const [expanded, setExpanded] = createSignal<Record<string, Node[]>>({})
  const [selected, setSelected] = createSignal<string>()
  const [content, setContent] = createSignal("")

  const refreshRoot = async () => {
    try {
      setTree(await invoke<Node[]>("list_dir", { rel: "" }))
    } catch {}
  }

  onMount(async () => {
    const ok = await api.waitReady()
    if (!ok) return setStatus("sidecar unreachable")
    const s = await api.createSession("downes")
    setSession(s.id)
    setReady(true)
    setStatus("ready · Downes")
    refreshRoot()
  })

  const activeIds = async (): Promise<string[]> => {
    try {
      const s = await api.server()
      const r = await fetch(s.url + "/api/session/active", {
        headers: { authorization: "Basic " + btoa(`${s.username}:${s.password}`) },
      })
      const list = await r.json()
      return (Array.isArray(list) ? list : []).map((x: any) => x.id ?? x.sessionID)
    } catch {
      return []
    }
  }

  const send = async (e: Event) => {
    e.preventDefault()
    const input = (e.currentTarget as HTMLFormElement).elements.namedItem("q") as HTMLInputElement
    const q = input.value.trim()
    if (!q || busy() || !session()) return
    input.value = ""
    setMsgs((m) => [...m, { role: "user", text: q }])
    setBusy(true)
    setStatus("Downes is working…")
    setMsgs((m) => [...m, { role: "assistant", text: "Planning and building the course… watch the files appear on the left." }])
    try {
      api.sendPrompt(session()!, q) // fire; we track completion by polling
      // poll: refresh tree + watch for idle
      let idleChecks = 0
      const tick = async () => {
        await refreshRoot()
        const active = await activeIds()
        if (session() && active.includes(session()!)) {
          idleChecks = 0
          setTimeout(tick, 2000)
        } else if (idleChecks < 2) {
          idleChecks++
          setTimeout(tick, 2000)
        } else {
          setBusy(false)
          setStatus("ready · Downes")
          setMsgs((m) => [...m, { role: "assistant", text: "Done. The course folder is on the left — open a file to view it." }])
        }
      }
      setTimeout(tick, 2500)
    } catch (err) {
      setBusy(false)
      setStatus("error sending prompt")
    }
  }

  const toggleDir = async (n: Node) => {
    const ex = expanded()
    if (ex[n.path]) {
      const { [n.path]: _, ...rest } = ex
      setExpanded(rest)
      return
    }
    try {
      const kids = await invoke<Node[]>("list_dir", { rel: n.path })
      setExpanded({ ...ex, [n.path]: kids })
    } catch {}
  }

  const openFile = async (n: Node) => {
    setSelected(n.path)
    try {
      setContent(await invoke<string>("read_file", { rel: n.path }))
    } catch (e) {
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

  return (
    <div class="app">
      <div class="topbar">
        <span class="brand">sage.is <small>mini · Downes studio</small></span>
        <span class={`status ${ready() ? "ok" : ""}`}>{status()}</span>
      </div>

      <div class="pane files">
        <h2>Courses</h2>
        <Show when={tree().length} fallback={<div class="empty">No courses yet.</div>}>
          <For each={tree()}>{(n) => <TreeRow n={n} />}</For>
        </Show>
      </div>

      <div class="chat">
        <div class="messages">
          <For each={msgs()}>{(m) => <div class={`msg ${m.role}`}>{m.text}</div>}</For>
          <Show when={!msgs().length}>
            <div class="empty">Ask Downes to design a course, lesson, or assessment.</div>
          </Show>
        </div>
        <form class="composer" onSubmit={send}>
          <input name="q" placeholder="Design a 4-week maker course for grade 8" disabled={!ready() || busy()} autocomplete="off" />
          <button type="submit" disabled={!ready() || busy()}>{busy() ? "…" : "Ask"}</button>
        </form>
      </div>

      <div class="pane viewer">
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
