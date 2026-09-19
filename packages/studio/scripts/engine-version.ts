#!/usr/bin/env bun
// The version the engine reports, and the one it sends as its User-Agent.
//
// Left alone, the build script stamps 0.0.0-<channel>-<timestamp> for every
// channel but "latest" (packages/script/src/index.ts). That is not a version
// any gate accepts, and providers do check: OpenCode's free tier answered
// "OpenCode's free tier can only be used from within OpenCode".
//
// So we report upstream's number, unqualified. This IS that engine: our
// patches are branding, a distinct state directory, and fixes that apply
// upstream too. When the fork diverges substantially — our own TUI, our own
// engine behaviour — append build metadata here and wear the difference:
//
//   console.log(`${pkg.version}+downes.${tauri.version}`)
//
// Build metadata (+), never a prerelease (-): 1.18.18-downes sorts BELOW
// 1.18.18 and is excluded from ranges like >=1.17.0 by default.
//
// One source for both platforms. macOS reads it from scripts/package_macos.sh
// in the Downes checkout; a Windows build exports it by hand (see
// docs/porting-windows.md). Neither hardcodes the number.

import pkg from "../../opencode/package.json"

console.log(pkg.version)
