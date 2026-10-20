// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Import-boundary check for the new trees (Architecture > Import rules).
// Backstops the oxlint no-restricted-imports config with the rules oxlint
// cannot express: layer direction, feature index.ts, network/TAuri touch
// points, the heavy-dependency import() rule, and the parity-env touch
// point (suite URLs derive through helpers/parity-env.ts).
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, posix, relative, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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

// Collected inside main() so importing this module (e.g. from the
// self-test) has no side effect; see the CLI guard at the bottom.
function collectSourceFiles() {
  for (const tree of ["apps/web_new", "packages/kit", "packages/api-client"]) {
    walk(join(root, tree));
  }
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

// A feature imports another feature only through its barrel: index.ts on
// disk, spelled index.js in import specifiers per the repo's ESM style
// (NEWFRONT-498). Both spellings below name the same file, so both pass.
// Pure: takes src-relative posix paths, returns the failure message or null
// when the import is allowed.
function crossFeatureFailure(fromSrcRel, toSrcRel) {
  const toParts = toSrcRel.split("/");
  const toFeature = toParts[1];
  const fromFeature = fromSrcRel.split("/")[1];
  if (toFeature === undefined || toFeature === fromFeature) return null;
  if (toParts.length === 3 && (toParts[2] === "index.ts" || toParts[2] === "index.js")) return null;
  return `cross-feature import must go through features/${toFeature}/index.ts`;
}

// The palette catalogs under shared/shell/entries/ load with the lazy
// chunks; a module imported from both a route's eager closure and its
// lazy chunks collapses that route's TanStack component split and
// trips the size gate (NEWFRONT-287). Only the catalogs themselves,
// the shell barrel, and the lazily loaded updates dialog may reference
// them; eager components take shared constants from the eager-only
// leaf (see shared/shell/externalLinks.ts). Pure: takes src-relative
// posix paths, returns the failure message or null when the import is
// allowed.
function entriesCatalogFailure(fromSrcRel, toSrcRel) {
  const toParts = toSrcRel.split("/");
  if (toParts[0] !== "shared" || toParts[1] !== "shell" || toParts[2] !== "entries") return null;
  if (fromSrcRel.startsWith("shared/shell/entries/")) return null;
  if (fromSrcRel === "shared/shell/index.ts" || fromSrcRel === "shared/shell/index.js") return null;
  // UpdatesDialog rides React.lazy from the gated route, so its catalog
  // import never joins an eager closure; keep it lazily loaded.
  if (fromSrcRel === "shared/shell/UpdatesDialog.tsx") return null;
  return "shared/shell/entries/* must not be statically imported outside entries/, the shell barrel and UpdatesDialog";
}

// The Lucide set behind stored project icons is 100+ glyphs; the eager
// project marks (shell rail, page header, cards, pickers) are in the first
// paint's closure. Only the lazy ProjectIcon wrapper and the emblem
// picker's own chunk may import it statically, or the initial-JS size gate
// jumps by ~7 kB (NEWFRONT-572).
const GLYPHS = /^shared\/pickers\/projectIconGlyphs\.(tsx|ts|jsx|js)$/;
const DRAWS_GLYPHS = /^(shared\/pickers\/ProjectIcon\.tsx|features\/project-settings\/general\/)/;

function projectIconGlyphsFailure(fromSrcRel, toSrcRel) {
  if (!GLYPHS.test(toSrcRel) || GLYPHS.test(fromSrcRel) || DRAWS_GLYPHS.test(fromSrcRel)) return null;
  return "shared/pickers/projectIconGlyphs must stay out of eager closures (import it through ProjectIcon, or from the emblem picker)";
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
    // A feature imports another feature only through its barrel.
    if (fromLayer === "features" && toLayer === "features") {
      const fromSrcRel = relative(appSrc, full).split(sep).join(posix.sep);
      const failure = crossFeatureFailure(fromSrcRel, importerRel);
      if (failure !== null) failures.push(`${rel}: ${failure} ("${spec}")`);
    }
    // Palette catalogs stay out of eager closures (see
    // entriesCatalogFailure).
    {
      const fromSrcRel = relative(appSrc, full).split(sep).join(posix.sep);
      const entriesFailure = entriesCatalogFailure(fromSrcRel, importerRel);
      if (entriesFailure !== null) failures.push(`${rel}: ${entriesFailure} ("${spec}")`);
      const iconFailure = projectIconGlyphsFailure(fromSrcRel, importerRel);
      if (iconFailure !== null) failures.push(`${rel}: ${iconFailure} ("${spec}")`);
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

  // Parity suite URLs derive from PARITY_NS through helpers/parity-env.ts
  // (NEWFRONT-132). A hardcoded localhost fallback anywhere else points at
  // the wrong stack under a bare namespaced run, so it fails here instead
  // of drifting back in through a concurrent oracle PR (NEWFRONT-353).
  const parityTree = "apps/web_new/e2e/parity/";
  if (
    rel.startsWith(parityTree) &&
    rel !== parityTree + "helpers/parity-env.ts" &&
    /process\.env\[["']PARITY_(ORACLE|API)_URL["']\]\s*\?\?\s*["']http:\/\/localhost/.test(text)
  ) {
    failures.push(
      `${rel}: hardcoded PARITY_ORACLE_URL/PARITY_API_URL localhost default; derive through parityOracleUrl/parityApiUrl from helpers/parity-env.ts`
    );
  }
}

function main() {
  collectSourceFiles();
  for (const file of sourceFiles) checkFile(file);

  if (failures.length > 0) {
    console.error("Import boundary check failed:");
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log(`Import boundary check passed (${sourceFiles.length} files).`);
}

// Run as a CLI when invoked directly (`node .../check-boundaries.mjs`);
// stay a pure module when imported (e.g. by the self-test), so importing
// has no side effect.
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}

export { crossFeatureFailure, entriesCatalogFailure, projectIconGlyphsFailure };
