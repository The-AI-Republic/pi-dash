// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// License-header check for the new trees (F-01; final text in F-11/NEWFRONT-22).
// The single definition lives in apps/web_new/LICENSE_HEADER.txt. A source
// file passes when it mentions the tree (Pi Dash contributors) and the
// placeholder owner (NEWFRONT-2), and never carries the old AGPL identifier.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const trees = ["apps/web_new", "packages/kit", "packages/api-client"];
const extensions = new Set([".ts", ".tsx", ".mts", ".cts", ".mjs", ".cjs", ".css"]);
const skipDirs = new Set(["node_modules", "dist", ".turbo"]);
// Generated router tree; F-02 similarity check ignores generated files too.
const skipFiles = new Set(["routeTree.gen.ts", ".gitkeep"]);

const failures = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (skipDirs.has(entry) || skipFiles.has(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      walk(full);
    } else if ([...extensions].some((ext) => entry.endsWith(ext))) {
      check(full);
    }
  }
}

function check(full) {
  const text = readFileSync(full, "utf8");
  const rel = relative(root, full);
  // Built at runtime so this script does not literally contain the string it bans.
  const oldIdentifier = ["SPDX-License-Identifier:", "AGPL-3.0-only"].join(" ");
  if (text.includes(oldIdentifier)) {
    failures.push(`${rel}: carries the old AGPL identifier`);
    return;
  }
  if (!text.includes("Pi Dash contributors") || !text.includes("NEWFRONT-2")) {
    failures.push(`${rel}: missing the license header from LICENSE_HEADER.txt`);
  }
}

for (const tree of trees) walk(join(root, tree));

if (failures.length > 0) {
  console.error("License header check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("License header check passed.");
