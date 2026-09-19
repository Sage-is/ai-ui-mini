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

// Refuse an engine that is not the one this build claims to ship. The copy
// above takes whatever sits in dist/, and dist/ is not rebuilt by a shell-only
// change: a Windows 0.1.13 installer went out carrying the engine built twelve
// days earlier, still reporting 0.0.0-downes/v1-<timestamp>. package_macos.sh
// asserts the same thing in the Downes checkout; this is the Windows half.
const expected = (await Bun.$`bun ${path.join(studioDir, "scripts", "engine-version.ts")}`.text()).trim()
const reported = (await Bun.$`${source} --version`.text().catch(() => "")).trim()
if (reported !== expected) {
  console.error(
    [
      `stage-payload: the compiled engine reports ${reported || "nothing"}, expected ${expected}`,
      ``,
      `Rebuild it, from the fork root:`,
      `  OPENCODE_CHANNEL=downes/v1 OPENCODE_VERSION=${expected} bun packages/opencode/script/build.ts --single`,
      ``,
      `In PowerShell:`,
      `  $env:OPENCODE_CHANNEL = "downes/v1"`,
      `  $env:OPENCODE_VERSION = (bun packages/studio/scripts/engine-version.ts)`,
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

// Which product this bundle is. product_workspace() reads this marker; without
// one it falls back to the bundle identifier, and that fallback parses
// Info.plist — a file no Windows bundle has. So on Windows the marker is not
// an optimisation, it is the only channel: unmarked, "SAGE.IS mini" would name
// its window correctly and still write into ~/Downes.
const product = process.env["DOWNES_PRODUCT"] ?? "SAGE.ISmini"
await fs.writeFile(path.join(payloadDir, "product"), `${product}\n`, "utf8")
console.log(`stage-payload: product marker = ${product}`)

// The curriculum template, when the caller names one. Downes ships it; mini has
// none, and must not: ensure_studio treats its absence as "this is the bare
// platform". It lives in the Downes checkout, which is AGPL — this script only
// copies what it is pointed at, and never carries a copy of its own.
//
// courses/ is the teacher's folder and is created empty at first launch, so the
// sample courses in the source template stay out of the bundle.
const templateDir = process.env["DOWNES_TEMPLATE"]
if (templateDir) {
  const config = path.join(templateDir, "opencode.json")
  if (!(await fs.stat(config).catch(() => undefined))?.isFile()) {
    console.error(`stage-payload: DOWNES_TEMPLATE has no opencode.json: ${templateDir}`)
    process.exit(1)
  }
  const target = path.join(payloadDir, "studio")
  await fs.cp(templateDir, target, {
    recursive: true,
    filter: (src) => !path.relative(templateDir, src).startsWith(path.join(".downes", "courses")),
  })
  console.log(`stage-payload: staged curriculum template -> ${path.relative(forkDir, target)}`)
}

// The same marker beside the unbundled binary. `tauri build` leaves
// target/release/downes-studio.exe runnable in place and people do run it —
// resources are only copied into the INSTALLER, so without this that copy
// disagrees with the installed one about which product it is.
const releaseDir = path.join(studioDir, "src-tauri", "target", "release")
if (await fs.stat(releaseDir).then((s) => s.isDirectory()).catch(() => false)) {
  await fs.writeFile(path.join(releaseDir, "product"), `${product}\n`, "utf8")
}
