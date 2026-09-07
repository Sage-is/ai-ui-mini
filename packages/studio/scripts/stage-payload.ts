#!/usr/bin/env bun
// Stages the payload the packaged app carries into src-tauri/payload, from
// where tauri.windows.conf.json copies it into the bundle.
//
// The shell resolves its engine relative to current_exe() (engine_bin() in
// src-tauri/src/lib.rs), never from a source checkout. That is correct, and it
// means an installed copy — which has no fork above it — finds no engine unless
// the bundle ships one. On macOS the Homebrew cask stages that payload; on
// Windows the bundle is the only delivery mechanism there is, so it has to
// carry the engine itself.
//
// Only the engine is staged. The curriculum template is optional: with none
// present ensure_studio() writes a default opencode.json rather than failing.

import fs from "fs/promises"
import path from "path"

const studioDir = path.resolve(import.meta.dir, "..")
const forkDir = path.resolve(studioDir, "../..")
const payloadDir = path.join(studioDir, "src-tauri", "payload")

// Mirrors engine_target()/engine_exe() in src-tauri/src/lib.rs, which in turn
// mirror the directory names packages/opencode/script/build.ts emits. This
// stages the HOST engine; a cross-compiled bundle needs the engine built for
// the target it is bundled for.
const os = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "darwin" : process.platform
const arch = process.arch === "arm64" ? "arm64" : "x64"
const exe = process.platform === "win32" ? "opencode.exe" : "opencode"
const target = `opencode-${os}-${arch}`

const source = path.join(forkDir, "packages", "opencode", "dist", target, "bin", exe)
const destination = path.join(payloadDir, "bin", exe)

const stat = await fs.stat(source).catch(() => undefined)
if (!stat?.isFile()) {
  console.error(
    [
      `stage-payload: no compiled engine at ${source}`,
      ``,
      `Build it first, from the fork root:`,
      `  bun packages/opencode/script/build.ts --single`,
    ].join("\n"),
  )
  process.exit(1)
}

// Replaced wholesale: a stale engine from an earlier build is worse than none,
// because it ships and runs.
await fs.rm(payloadDir, { recursive: true, force: true })
await fs.mkdir(path.dirname(destination), { recursive: true })
await fs.copyFile(source, destination)

const mb = (stat.size / 1024 / 1024).toFixed(1)
console.log(`stage-payload: staged ${target}/bin/${exe} (${mb} MB) -> ${path.relative(forkDir, destination)}`)
