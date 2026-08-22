// Markdown → safe HTML, plus a lightweight reveal-style slide splitter for
// reveal.js decks (--- separated). Same libs session-ui uses under the hood.
import { marked } from "marked"
import DOMPurify from "dompurify"

marked.setOptions({ gfm: true, breaks: false })

export function md(src: string): string {
  return DOMPurify.sanitize(marked.parse(src, { async: false }) as string)
}

export type Slide = { body: string; notes: string }

// Split a reveal.js markdown deck on bare --- lines; pull out Notes: blocks.
export function slides(src: string): Slide[] {
  return src
    .split(/^\s*---\s*$/m)
    .map((chunk) => chunk.trim())
    .filter(Boolean)
    .map((chunk) => {
      const idx = chunk.search(/^Notes:/m)
      if (idx === -1) return { body: md(chunk), notes: "" }
      return {
        body: md(chunk.slice(0, idx).trim()),
        notes: chunk.slice(idx).replace(/^Notes:\s*/m, "").trim(),
      }
    })
}

export function isDeck(name: string, src: string): boolean {
  return /slides?\.md$/i.test(name) && /^\s*---\s*$/m.test(src)
}

export function isHtml(name: string): boolean {
  return /\.html?$/i.test(name)
}

// A tiny script injected into an HTML artifact. It does two jobs:
//   1. Neutralize the History API. The frame is sandboxed with no same-origin,
//      so its origin is opaque (about:srcdoc); a deck calling
//      history.replaceState to sync slide URLs would throw SecurityError.
//      No-op'd, reveal.js still navigates by keys/state — only URL sync is
//      lost, which is invisible in an embedded viewer.
//   2. Route anchor clicks to the default browser via postMessage — the
//      frame's only channel back to the shell (the app's own link handler
//      cannot reach a separate document).
// It must run before the deck's own scripts, so it is injected at the top of
// <head>. postMessage is the only channel out; the frame stays isolated.
const BRIDGE =
  "<script>try{history.replaceState=function(){};history.pushState=function(){};}catch(e){}" +
  "document.addEventListener('click',function(e){" +
  "var a=e.target.closest&&e.target.closest('a');" +
  "if(a&&a.href&&/^https?:/i.test(a.href)){e.preventDefault();" +
  "parent.postMessage({__downes_open:a.href},'*');}},true);<\/script>"

// Inject the bridge at the top of <head> (or <html>) so it runs before the
// artifact's own scripts. A fragment with neither just gets it prepended.
export function htmlDoc(src: string): string {
  if (/<head[^>]*>/i.test(src)) return src.replace(/<head[^>]*>/i, (m) => m + BRIDGE)
  if (/<html[^>]*>/i.test(src)) return src.replace(/<html[^>]*>/i, (m) => m + BRIDGE)
  return BRIDGE + src
}
