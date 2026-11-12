// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Size-limit budgets (Quality gates; run: pnpm --filter web_new size, after
// pnpm build:web). The JS budget covers shell + first route only: the chain
// is resolved from the web manifest, so lazy route chunks never move the
// reading and chunk hashes can rotate freely.
//
// One initial-JS ceiling, 276 KB gzipped: a reading above it fails the PR.
// It is sized to catch an accidental large eager import, not ordinary slice
// growth (NEWFRONT-248, 2026-10-15; raised from 260 KB on 2026-10-16 when
// shell growth used up the headroom, from 265 KB on 2026-10-17 when the
// measured issues foundation slice (NEWFRONT-614, +6.7 KB of route chunk
// plus documented eager leafs, no accidental eager import) landed at
// 267.6 KB, and from 270 KB on 2026-10-17 when the display-dropdown slice
// (NEWFRONT-618, header trigger necessarily eager, panel plus matrix
// lazy-split with the panel chunk out of the initial chain, no accidental
// eager import) measured 270.68 KB in CI, and from 273 KB on 2026-10-18 when
// the desktop runtime lifecycle slice (NEWFRONT-552, root-mounted sign-out
// registration and the UI-free prompt bridge necessarily eager, dialog and
// runtime service lazy-split, no accidental eager import) measured 273.58 KB
// locally on the current base; CI reads about half a KB higher, so 274 KB
// would leave no margin, and from 275 KB on 2026-11-04 when the
// overflow-panel slice (NEWPIDASH-9, the sidebar row's quick menu and section
// sub-nav extracted into shared components used by the row and the lazy
// panel, panel chunk out of the initial chain, no accidental eager import, no
// new dependencies) measured 275.08 KB in CI, matched locally to the byte).
//
// There is no second number and no environment variable to switch budgets:
// owner decision 5 of 2026-11-10 retired the earlier composition target, so a
// reading under the former target is no longer reported or followed up
// (NEWFRONT-1061). Initial CSS has its own ceiling, 30 KB gzipped.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { sizeLimits } from "./checks/size-budget.mjs";
import { MANIFEST_PATH, initialChainFiles } from "./checks/size-initial.mjs";

const appRoot = dirname(fileURLToPath(import.meta.url));
const manifestPath = join(appRoot, MANIFEST_PATH);

if (!existsSync(manifestPath)) {
  throw new Error(`size gate: missing ${MANIFEST_PATH}; run "pnpm build:web" before "pnpm size".`);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const initialJs = initialChainFiles(manifest);

// Size-limit globs paths silently: a file that stopped matching would pass
// vacuously, so every resolved chunk must exist on disk.
for (const file of initialJs) {
  if (!existsSync(join(appRoot, file))) {
    throw new Error(`size gate: the manifest lists "${file}" but it is not on disk.`);
  }
}

export default sizeLimits(initialJs);
