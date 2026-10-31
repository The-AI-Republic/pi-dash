// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Size-limit budgets (Quality gates; run: pnpm --filter web_new size, after
// pnpm build:web). The JS budget covers shell + first route only: the chain
// is resolved from the web manifest, so lazy route chunks never move the
// reading and chunk hashes can rotate freely.
//
// Initial JS has two numbers. The target, 220 KB gzipped (NEWFRONT-196), is
// what the app is composed to fit; CI reports it on every PR but does not
// block on it, and area gates file a diet issue when it is exceeded. The
// ceiling, 275 KB, is what fails a PR: it catches an accidental large eager
// import, not ordinary slice growth (NEWFRONT-248, 2026-10-15; raised from
// 260 KB on 2026-10-16 when shell growth used up the headroom, from
// 265 KB on 2026-10-17 when the measured issues foundation slice
// (NEWFRONT-614, +6.7 KB of route chunk plus documented eager leafs, no
// accidental eager import) landed at 267.6 KB, and from 270 KB on
// 2026-10-17 when the display-dropdown slice (NEWFRONT-618, header
// trigger necessarily eager, panel plus matrix lazy-split with the panel
// chunk out of the initial chain, no accidental eager import) measured
// 270.68 KB in CI, and from 273 KB on 2026-10-18 when the desktop runtime
// lifecycle slice (NEWFRONT-552, root-mounted sign-out registration and the
// UI-free prompt bridge necessarily eager, dialog and runtime service
// lazy-split, no accidental eager import) measured 273.58 KB locally on the
// current base; CI reads about half a KB higher, so 274 KB would leave no
// margin.
// Set SIZE_TARGET=1 to
// measure against the target. Initial CSS: 30 KB gzipped.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { MANIFEST_PATH, initialChainFiles } from "./checks/size-initial.mjs";

const appRoot = dirname(fileURLToPath(import.meta.url));
const manifestPath = join(appRoot, MANIFEST_PATH);

if (!existsSync(manifestPath)) {
  throw new Error(`size gate: missing ${MANIFEST_PATH}; run "pnpm build:web" before "pnpm size".`);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const initialJs = initialChainFiles(manifest);
const initialJsLimit = process.env.SIZE_TARGET === "1" ? "220 KB" : "275 KB";

// Size-limit globs paths silently: a file that stopped matching would pass
// vacuously, so every resolved chunk must exist on disk.
for (const file of initialJs) {
  if (!existsSync(join(appRoot, file))) {
    throw new Error(`size gate: the manifest lists "${file}" but it is not on disk.`);
  }
}

export default [
  {
    name: "web initial JS (shell + first route)",
    path: initialJs,
    limit: initialJsLimit,
    gzip: true,
  },
  {
    name: "web initial CSS",
    path: "dist/web/assets/*.css",
    limit: "30 KB",
    gzip: true,
  },
];
