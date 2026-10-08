// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// License-header check for the new trees (F-01; interim AGPL text in F-11/NEWFRONT-22).
// The single definition lives in apps/web_new/LICENSE_HEADER.txt. A source
// file passes when it carries the three header lines (copyright, SPDX
// identifier, LICENSE pointer) and never the old tree's copyright line,
// which trips the copied-file wire.
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
  const oldCopyright = ["Copyright (c) 2023-present", "Pi Dash Software, Inc. and contributors"].join(" ");
  if (text.includes(oldCopyright)) {
    failures.push(`${rel}: carries the old tree's copyright line (copied file?)`);
    return;
  }
  const required = [
    "Copyright (c) Pi Dash contributors",
    "SPDX-License-Identifier: AGPL-3.0-only",
    "See the LICENSE file for details.",
  ];
  const missing = required.filter((line) => !text.includes(line));
  if (missing.length > 0) {
    failures.push(`${rel}: missing the license header from LICENSE_HEADER.txt (${missing.join("; ")})`);
  }
}

for (const tree of trees) walk(join(root, tree));

if (failures.length > 0) {
  console.error("License header check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("License header check passed.");
