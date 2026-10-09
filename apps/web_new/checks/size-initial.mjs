// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Resolves the initial-JS route chain from the web build manifest, for the
// size gate (.size-limit.mjs). The Quality gates budget covers "shell +
// first route": the entry chunk plus the route components the issue-list
// first paint loads. Lazy splits (sign-in, error/notFound fallbacks, other
// routes) are excluded, so landing a new route cannot move the reading.
// Chunk hashes rotate every build; this keys on stable manifest entries
// (index.html + route source paths) instead (NEWFRONT-190).
import { posix } from "node:path";

export const WEB_OUT_DIR = "dist/web";
export const MANIFEST_PATH = "dist/web/.vite/manifest.json";
export const ENTRY_KEY = "index.html";

// Manifest keys of the route components on the issue-list first paint: the
// $ws layout and the issues leaf. TanStack autoCodeSplitting emits one
// dynamic split per key; errorComponent/notFoundComponent splits and every
// other route stay lazy and are excluded by design.
export const FIRST_ROUTE_KEYS = [
  "src/routes/$ws/route.tsx?tsr-split=component",
  "src/routes/$ws/projects/$projectId/issues/index.tsx?tsr-split=component",
];

function assertEntry(manifest, key, role) {
  const entry = manifest[key];
  if (entry === undefined) {
    throw new Error(
      `size gate: ${role} manifest key "${key}" is missing. ` +
        "The route split moved or was renamed — update FIRST_ROUTE_KEYS in checks/size-initial.mjs."
    );
  }
  if (typeof entry !== "object" || typeof entry.file !== "string" || entry.file === "") {
    throw new Error(`size gate: manifest entry "${key}" carries no output file.`);
  }
  return entry;
}

// Static closure of the entry + first-route roots, as app-root-relative
// posix paths (e.g. "dist/web/assets/index-AbC123.js"), sorted and deduped.
// Follows `imports` only: `dynamicImports` are lazy routes, excluded.
// Throws on any unknown key so a stale route list fails the gate loudly
// instead of silently shrinking the measured set.
export function initialChainFiles(
  manifest,
  { entryKey = ENTRY_KEY, routeKeys = FIRST_ROUTE_KEYS, outDir = WEB_OUT_DIR } = {}
) {
  if (manifest === null || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error("size gate: the vite manifest is not an object.");
  }
  const files = new Set();
  const visited = new Set();
  const queue = [entryKey, ...routeKeys];
  // Validate every root up front, so a renamed route fails even when the
  // entry closure alone would already cover the page.
  assertEntry(manifest, entryKey, "entry");
  for (const key of routeKeys) assertEntry(manifest, key, "first-route");
  while (queue.length > 0) {
    const key = queue.pop();
    if (visited.has(key)) continue;
    visited.add(key);
    const entry = manifest[key];
    if (entry === undefined) {
      throw new Error(`size gate: manifest key "${key}" is imported but not emitted.`);
    }
    files.add(posix.join(outDir, entry.file));
    for (const next of entry.imports ?? []) queue.push(next);
  }
  return [...files].sort();
}
