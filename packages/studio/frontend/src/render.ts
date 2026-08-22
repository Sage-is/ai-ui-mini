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
