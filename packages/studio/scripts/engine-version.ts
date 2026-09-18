#!/usr/bin/env bun
// The version the engine reports, and the one it sends as its User-Agent.
//
// Left alone, the build script stamps 0.0.0-<channel>-<timestamp> for every
// channel but "latest" (packages/script/src/index.ts), so our engine announced
// itself as 0.0.0 — older than any gate asking for a minimum opencode version,
// and providers do ask. Upstream's number is the honest one to report: this IS
// that engine, with our patches on top.
//
// Build metadata (+), never a prerelease (-): 1.18.18-downes.0.1.13 sorts BELOW
// 1.18.18 for every semver comparator and is excluded from ranges like >=1.17.0
// by default, which is the exact failure this exists to fix.
//
// One source for both platforms. macOS reads it from scripts/package_macos.sh
// in the Downes checkout; a Windows build exports it by hand (see
// docs/porting-windows.md). Neither hardcodes the number.

import pkg from "../../opencode/package.json"
import tauri from "../src-tauri/tauri.conf.json"

console.log(`${pkg.version}+downes.${tauri.version}`)
