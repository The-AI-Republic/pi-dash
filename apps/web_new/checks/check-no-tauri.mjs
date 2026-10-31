// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Web-bundle check (F-04): the PIDASH_TARGET=web bundle must contain no
// desktop code. Scans the built web assets for the markers only
// core/platform/tauri.ts can emit. Run after `build:web`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const webDir = join(root, "dist", "web");

// Tokens only the desktop platform module can emit. Bare words like
// "desktop" are deliberately absent: the target switch itself mentions
// them in both bundles.
const markers = [
  "__TAURI__",
  "__PIDASH_NATIVE_HTTP__",
  "desktop_api_request",
  "desktop_api_stream",
  "desktop_api_cancel",
  "tauri-apps",
  "plugin:opener",
];

function collectJs(dir) {
  const files = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      files.push(...collectJs(full));
    } else if (entry.endsWith(".js")) {
      files.push(full);
    }
  }
  return files;
}

let webStat;
try {
  webStat = statSync(webDir);
} catch {
  console.error("No-Tauri check failed: apps/web_new/dist/web is missing. Run `pnpm build:web` first.");
  process.exit(1);
}
if (!webStat.isDirectory()) {
  console.error("No-Tauri check failed: apps/web_new/dist/web is not a directory.");
  process.exit(1);
}

const failures = [];
for (const file of collectJs(webDir)) {
  const text = readFileSync(file, "utf8");
  for (const marker of markers) {
    if (text.includes(marker)) {
      failures.push(`${file}: web bundle contains desktop marker "${marker}"`);
    }
  }
}

if (failures.length > 0) {
  console.error("No-Tauri check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("No-Tauri check passed: the web bundle contains no desktop code.");
