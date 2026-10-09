// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Size-limit budgets (Quality gates; run: pnpm --filter web_new size, after
// pnpm build:web). The JS budget covers shell + first route only: the chain
// is resolved from the web manifest, so lazy route chunks never move the
// reading and chunk hashes can rotate freely. Budgets: initial JS 220 KB
// gzipped (NEWFRONT-196), initial CSS 30 KB gzipped.
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
    limit: "220 KB",
    gzip: true,
  },
  {
    name: "web initial CSS",
    path: "dist/web/assets/*.css",
    limit: "30 KB",
    gzip: true,
  },
];
