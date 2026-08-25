import { createSignal, onMount, Show } from "solid-js"

// Light/dark toggle following startr.style's mechanism: a data-theme
// attribute on <html>, persisted under localStorage "theme". The initial
// apply happens in index.html before the body parses (no flash); this
// component only reflects and changes it.
//
// Three states, cycled by clicking: system -> light -> dark -> system.
// "system" tracks the OS live — the matchMedia listener lives in index.html.

type Pref = "system" | "light" | "dark"

declare global {
  interface Window {
    __applyTheme?: (pref: Pref) => void
  }
}

export default function ThemeToggle() {
  const [pref, setPref] = createSignal<Pref>("system")

  onMount(() => {
    let stored: string | null = null
    try {
      stored = localStorage.getItem("theme")
    } catch {}
    setPref((stored as Pref) || "system")
  })

  const cycle = () => {
    const next: Pref = pref() === "system" ? "light" : pref() === "light" ? "dark" : "system"
    setPref(next)
    try {
      // "system" is stored as an explicit value, not a missing key, so the
      // bootstrap can tell "follow the OS" from "never chose".
      localStorage.setItem("theme", next)
    } catch {}
    window.__applyTheme?.(next)
  }

  const title = () =>
    pref() === "system"
      ? "Theme: following system — click for light"
      : pref() === "light"
        ? "Theme: light — click for dark"
        : "Theme: dark — click to follow system"

  return (
    <button class="theme-toggle" onClick={cycle} title={title()} aria-label={title()}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <Show when={pref() === "light"}>
          {/* sun */}
          <path d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" />
          <path d="M12 4v1M17.66 6.34l-.83.83M20 12h-1M17.66 17.66l-.83-.83M12 20v-1M6.34 17.66l.83-.83M4 12h1M6 6l.84.84" />
        </Show>
        <Show when={pref() === "dark"}>
          {/* moon */}
          <path
            class="fill-moon"
            d="M17.7 15.15A6.5 6.5 0 0 1 9 6.04 7.14 7.14 0 0 0 4 12.87 7.09 7.09 0 0 0 11.04 20a7.03 7.03 0 0 0 6.67-4.85Z"
          />
        </Show>
        <Show when={pref() === "system"}>
          {/* display — "follow the OS" */}
          <rect x="3" y="5" width="18" height="12" rx="2" />
          <path d="M8 20h8M12 17v3" />
        </Show>
      </svg>
    </button>
  )
}
