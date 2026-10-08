// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Import-boundary check for the new trees (Architecture > Import rules).
// Backstops the oxlint no-restricted-imports config with the rules oxlint
// cannot express: layer direction, feature index.ts, network/TAuri touch
// points, and the heavy-dependency import() rule.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, posix, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const appSrc = join(root, "apps", "web_new", "src");

const failures = [];

// --- specifier bans (old frontend + heavy static imports) -----------------

const bannedExact = new Set(["@pi-dash/ui", "apps/web", "apps/admin", "apps/space"]);

const bannedPrefixes = [
  "@pi-dash/",
  "apps/web/",
  "apps/admin/",
  "apps/space/",
  "desktop-overlay/",
  "ee-overlay/",
  "ce/",
];

// Heavy dependencies are only reached through import() (Stack). Static
// imports of them fail here; dynamic import() is intentionally allowed.
const heavyPrefixes = [
  "@tiptap/",
  "tiptap",
  "recharts",
  "yjs",
  "jspdf",
  "@atlaskit/pragmatic-drag-and-drop",
  "emoji-mart",
  "@emoji-mart/",
];

function bannedSpecifier(spec) {
  if (bannedExact.has(spec)) return "old frontend module";
  for (const prefix of bannedPrefixes) {
    if (spec === prefix || spec.startsWith(prefix)) return "old frontend path";
  }
  if (spec.includes("apps/web") || spec.includes("apps/admin") || spec.includes("apps/space")) {
    return "old frontend path";
  }
  return null;
}

function heavySpecifier(spec) {
  return heavyPrefixes.some((prefix) => spec === prefix || spec.startsWith(prefix));
}

// --- file collection -------------------------------------------------------

const sourceFiles = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (entry === "node_modules" || entry === "dist" || entry === ".turbo") continue;
    if (statSync(full).isDirectory()) {
      walk(full);
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      if (!entry.endsWith(".gen.ts") && !entry.endsWith(".test.ts") && !entry.endsWith(".test.tsx")) {
        sourceFiles.push(full);
      }
    }
  }
}

for (const tree of ["apps/web_new", "packages/kit", "packages/api-client"]) {
  walk(join(root, tree));
}

// --- per-file checks -------------------------------------------------------

const staticImport =
  /(?:import|export)\s+[^"']*?from\s*["']([^"']+)["']|import\s*["']([^"']+)["']|require\(\s*["']([^"']+)["']\s*\)/g;

function toSrcRelative(importer, spec) {
  if (!spec.startsWith(".")) return null;
  const resolved = join(importer, "..", spec);
  const rel = relative(appSrc, resolved).split(sep).join(posix.sep);
  if (rel.startsWith("..")) return null;
  return rel;
}

function layerOf(srcRel) {
  const head = srcRel.split("/")[0];
  if (["routes", "features", "shared", "core"].includes(head)) return head;
  return "other";
}

function checkFile(full) {
  const text = readFileSync(full, "utf8");
  const rel = relative(root, full).split(sep).join(posix.sep);
  const inAppSrc = full.startsWith(appSrc + sep);

  for (const match of text.matchAll(staticImport)) {
    const spec = match[1] ?? match[2] ?? match[3];
    const reason = bannedSpecifier(spec);
    if (reason !== null) {
      failures.push(`${rel}: banned import "${spec}" (${reason})`);
      continue;
    }
    if (heavySpecifier(spec)) {
      failures.push(`${rel}: heavy dependency "${spec}" must be reached through import(), not a static import`);
      continue;
    }
    // Relative imports that escape the new trees can smuggle old code in.
    if (spec.startsWith(".")) {
      const resolved = join(full, "..", spec);
      const outside =
        !resolved.startsWith(join(root, "apps", "web_new") + sep) &&
        !resolved.startsWith(join(root, "packages", "kit") + sep) &&
        !resolved.startsWith(join(root, "packages", "api-client") + sep);
      if (outside) {
        failures.push(`${rel}: relative import "${spec}" escapes the new trees`);
        continue;
      }
    }
    if (!inAppSrc) continue;
    const importerRel = toSrcRelative(full, spec);
    if (importerRel === null) continue;
    const fromLayer = layerOf(relative(appSrc, full).split(sep).join(posix.sep));
    const toLayer = layerOf(importerRel);
    if (fromLayer === "other" || toLayer === "other") continue;
    // A layer imports only from layers below it:
    // routes > features > shared > core.
    const rank = { routes: 3, features: 2, shared: 1, core: 0 };
    if (rank[fromLayer] < rank[toLayer]) {
      failures.push(`${rel}: ${fromLayer} must not import from ${toLayer} ("${spec}")`);
      continue;
    }
    // A feature imports another feature only through its index.ts.
    if (fromLayer === "features" && toLayer === "features") {
      const fromFeature = importerRel.split("/")[1];
      const toParts = importerRel.split("/");
      const toFeature = toParts[1];
      const fromSelf = relative(appSrc, full).split(sep).join(posix.sep).split("/")[1];
      if (toFeature !== fromSelf && !(toParts.length === 3 && toParts[2] === "index.ts") && toFeature !== undefined) {
        void fromFeature;
        failures.push(`${rel}: cross-feature import must go through features/${toFeature}/index.ts ("${spec}")`);
      }
    }
  }

  // Only core/api performs network I/O; only core/platform touches Tauri.
  if (inAppSrc) {
    const srcRel = relative(appSrc, full).split(sep).join(posix.sep);
    const inApi = srcRel.startsWith("core/api/");
    const inPlatform = srcRel.startsWith("core/platform/");
    if (!inApi && !inPlatform && (/\bfetch\s*\(/.test(text) || /new\s+EventSource\s*\(/.test(text))) {
      failures.push(`${rel}: network I/O is only allowed in core/api (and platform transports)`);
    }
    if (!inPlatform && (/__TAURI__/.test(text) || /@tauri-apps\//.test(text))) {
      failures.push(`${rel}: Tauri access is only allowed in core/platform`);
    }
  }
}

for (const file of sourceFiles) checkFile(file);

if (failures.length > 0) {
  console.error("Import boundary check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`Import boundary check passed (${sourceFiles.length} files).`);
