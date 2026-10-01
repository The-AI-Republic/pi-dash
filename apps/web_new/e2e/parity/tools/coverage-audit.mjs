#!/usr/bin/env node
/**
 * Coverage audit: every route, component, service method and shortcut of the
 * old frontends must appear in some feature inventory.
 *
 * What it does (read-only against the old apps; never imports them):
 *  1. Enumerates, from the old frontends:
 *     - route files (page.tsx, layout.tsx, route modules, routes.ts entries)
 *       in apps/web/app, apps/admin/app, apps/space/app,
 *       desktop-overlay/apps/web/app and the cloud overlay
 *       ee-overlay/apps/{web,admin}/app (when present);
 *     - component folders two levels deep under core/components and
 *       ce/components (and the admin/space/desktop/cloud equivalents);
 *     - public methods of API service classes with HTTP verb + path fragment;
 *     - keyboard shortcut registrations (palette command configs, key bindings);
 *     - redirects and error/not-found routes.
 *  2. Checks each item against every file in
 *     .ai_design/new_frontend_implementation/parity/inventory/:
 *     covered (checklist or row text names it), explained (an inventory's
 *     explained/no-row section names it), or unclaimed.
 *  3. Writes .ai_design/new_frontend_implementation/parity/coverage-audit.md.
 *
 * Report-only tool: exits 0 even with unclaimed items (CI runs it next to the
 * parity report, never as a blocking check). Pass --strict to exit non-zero
 * when unclaimed items remain (useful locally before editing inventories).
 *
 * Usage:
 *   node coverage-audit.mjs [--root <repo-root>]
 *     [--cloud-overlay <dir>] [--out <report-path>] [--strict] [--help]
 *
 * --cloud-overlay defaults to $CLOUD_OVERLAY_DIR when set; when the directory
 * is absent the cloud section of the report is marked "source unavailable"
 * instead of failing, so CI (which has no access to the private overlay repo)
 * still produces a report for the local sources.
 */

import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, relative, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(HERE, "..", "..", "..", "..");
const DEFAULT_OUT = join(
  ".ai_design",
  "new_frontend_implementation",
  "parity",
  "coverage-audit.md"
);

/* ------------------------------------------------------------------ */
/* small fs helpers                                                    */
/* ------------------------------------------------------------------ */

function walk(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    if (e.name === "node_modules" || e.name.startsWith(".")) continue;
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (e.isFile()) out.push(p);
  }
  return out;
}

function read(p) {
  try {
    return readFileSync(p, "utf8");
  } catch {
    return null;
  }
}

/** Repo-relative posix path, or null when outside the root. */
function rel(root, p) {
  const r = relative(root, p).split("\\").join("/");
  return r.startsWith("..") ? null : r;
}

/** Strip Next.js route groups: "app/(all)/x/(detail)/y" -> "app/x/y". */
function stripGroups(p) {
  return p.replace(/\/\([^/]*\)/g, "");
}

/** Structural dir names that are never area-distinctive on their own. */
const GENERIC_DIRS = new Set([
  "app", "src", "lib", "core", "ce", "ee", "components", "routes", "store", "hooks", "types", "utils",
]);

/* ------------------------------------------------------------------ */
/* enumerators (old frontends, read-only)                              */
/* ------------------------------------------------------------------ */

const ROUTE_FILES = new Set([
  "page.tsx",
  "layout.tsx",
  "route.ts",
  "route.tsx",
  "default.tsx",
  "not-found.tsx",
  "error.tsx",
  "global-error.tsx",
]);

/** Public methods of `export class *Service` with verb + path fragment. */
function enumerateServicesFromFiles(root, files) {
  const items = [];
  const seen = new Set();
  for (const f of files) {
    const r = rel(root, f) || f;
    if (seen.has(r)) continue;
    seen.add(r);
    const text = read(f);
    if (!text) continue;
    const classRe = /export\s+class\s+(\w+)/g;
    let cm;
    while ((cm = classRe.exec(text))) {
      const cls = cm[1];
      let body = text.slice(cm.index);
      const tail = body.slice(1).search(/\nexport\s/);
      if (tail >= 0) body = body.slice(0, tail + 1);
      // Scan the class body for async method definitions.
      const methods = [...body.matchAll(/(?:^|\n)\s*async\s+([A-Za-z_]\w*)\s*\(/g)]
        .map((m) => m[1])
        .filter((n) => n !== "constructor");
      for (const name of new Set(methods)) {
        const at = body.indexOf(`async ${name}(`);
        // Bound the slice at the next method so literals/verbs cannot bleed
        // across methods (a bare method would otherwise inherit its neighbor's
        // endpoint and produce false evidence).
        const next = body.indexOf("\n  async ", at + 1);
        const slice = body.slice(at, next > 0 ? next : at + 1500);
        const verb = (/this\.(get|post|put|patch|delete)\s*\(/.exec(slice) || [])[1] || "unknown";
        // Every '/'-bearing literal is a candidate endpoint fragment; the first
        // is shown in the id, all participate in matching. Closed
        // `${placeholders}` collapse; a literal cut off inside an
        // interpolation (ternary URLs, query builders) ends there.
        const lits = [...slice.matchAll(/["'`]([^"'`]{1,160})["'`]/g)]
          .map((m) => m[1].replace(/\$\{[^}]*\}/g, "{}").split("${")[0])
          .filter((s) => s.includes("/") && /[a-z0-9]/i.test(s));
        const pathFrag = lits[0] || "";
        const keys = [`${cls}.${name}`];
        if (/[A-Z]/.test(name.slice(1)) || name.length >= 8) keys.push(name);
        for (const lit of new Set(lits).size ? [...new Set(lits)].slice(0, 6) : []) keys.push(lit);
        items.push({
          category: "service-methods",
          kind: "service-method",
          id: `${r} :: ${cls}.${name} [${verb}${pathFrag ? " " + pathFrag : ""}]`,
          keys,
        });
      }
    }
  }
  return items;
}

/** Palette command configs + key-binding registrations. */
function enumerateShortcuts(root, cloudRoot) {
  const items = [];
  const bases = ["apps/web", "apps/admin", "apps/space", "desktop-overlay/apps/web"];
  if (cloudRoot) bases.push(`${cloudRoot}/ee-overlay/apps/web`);
  for (const base of bases) {
    const abs = base.startsWith("/") || base.startsWith(".") ? base : join(root, base);
    const baseForRel = abs.startsWith(root) ? root : null;
    for (const f of walk(abs)) {
      const r = baseForRel ? rel(baseForRel, f) : f;
      if (!r || !/\.(ts|tsx)$/.test(r)) continue;
      const low = r.toLowerCase();
      const isPalette = /power-k\/(config|core|menus)|command-palette|global-shortcuts|keybinding|hotkey|shortcut/.test(low);
      if (!isPalette) continue;
      const text = read(f) || "";
      const keys = [r.split("/").pop()];
      // Command ids / titles: id: '...', title: '...', label: '...'.
      const lits = [...text.matchAll(/(?:id|title|label|command)\s*:\s*["'`]([^"'`]{2,60})["'`]/g)]
        .map((m) => m[1])
        .filter((s) => /[a-zA-Z]/.test(s));
      for (const lit of new Set(lits)) keys.push(lit);
      // Key combos: endeavor to catch "cmd+k", "ctrl+p", "?" style bindings.
      const combos = [...text.matchAll(/["'`](?:(?:cmd|ctrl|shift|alt|meta|mod)\s*\+\s*){1,3}[a-z0-9?/[\]\\-](?:\s*\+\s*[a-z0-9?/[\]\\-])*["'`]/gi)]
        .map((m) => m[0].slice(1, -1));
      for (const c of new Set(combos)) keys.push(c);
      items.push({ category: "shortcuts", kind: "shortcut-site", id: r, keys });
    }
  }
  return items;
}

/** Redirects, catch-alls and error/not-found routes. */
function enumerateRedirects(root, cloudRoot) {
  const items = [];
  const push = (id, keys) => items.push({ category: "redirects-errors", kind: "redirect", id, keys });
  const redirectFiles = [];
  for (const f of walk(join(root, "apps/web/app/routes"))) {
    const r = rel(root, f);
    if (r && /\.(ts|tsx)$/.test(r)) redirectFiles.push({ f, r });
  }
  for (const { r } of redirectFiles) push(r, [r, stripGroups(r)]);
  for (const appDir of ["apps/web/app", "apps/admin/app", "apps/space/app", "desktop-overlay/apps/web/app"]) {
    for (const f of walk(join(root, appDir))) {
      const r = rel(root, f);
      if (!r) continue;
      const base = f.split("/").pop();
      if (base === "not-found.tsx" || base === "error.tsx" || base === "global-error.tsx") {
        push(r, [r, stripGroups(r)]);
      } else if ((base === "page.tsx" || base === "layout.tsx" || base === "route.ts") && !r.includes("/routes/")) {
        const text = read(f) || "";
        if (/\bredirect\(|permanentRedirect|notFound\(\)/.test(text)) push(r, [r, stripGroups(r)]);
      }
    }
    if (appDir === "apps/admin/app" || appDir === "apps/space/app") {
      const cfg = join(root, appDir, "routes.ts");
      if (existsSync(cfg)) {
        const text = read(cfg) || "";
        if (/route\("\*",/.test(text)) {
          const r = rel(root, cfg);
          push(`${r} :: catch-all "*"`, [r, "catch-all", '"*"']);
        }
      }
    }
  }
  if (cloudRoot) {
    for (const appDir of ["ee-overlay/apps/web/app", "ee-overlay/apps/admin/app"]) {
      for (const f of walk(join(cloudRoot, appDir))) {
        const base = f.split("/").pop();
        if (base === "page.tsx" || base === "layout.tsx") {
          const text = read(f) || "";
          if (/\bredirect\(|permanentRedirect|notFound\(\)/.test(text)) {
            const r = `${cloudRoot}/${appDir}/${relative(join(cloudRoot, appDir), f)}`;
            push(r, [r.split("/").slice(-4).join("/"), stripGroups(r)]);
          }
        }
      }
    }
  }
  return items;
}

/* ------------------------------------------------------------------ */
/* inventory parsing                                                   */
/* ------------------------------------------------------------------ */

/** Reduce an item path to the area-relative basis inventories quote. */
function areaPath(r) {
  let s = String(r);
  const cut = (marker) => {
    const i = s.indexOf(marker);
    if (i >= 0) s = s.slice(i + marker.length);
  };
  cut("ee-overlay/apps/web/app/");
  cut("ee-overlay/apps/admin/app/");
  cut("ee-overlay/apps/web/");
  cut("ee-overlay/apps/admin/");
  const roots = [
    "apps/web/app/(all)/[workspaceSlug]/(projects)/",
    "apps/web/app/(all)/[workspaceSlug]/(settings)/",
    "apps/web/app/(all)/[workspaceSlug]/",
    "apps/web/app/",
    "apps/web/",
    "apps/admin/app/",
    "apps/admin/",
    "apps/space/app/",
    "apps/space/",
    "desktop-overlay/apps/web/app/",
    "desktop-overlay/apps/web/",
  ];
  for (const root of roots) {
    if (s.toLowerCase().startsWith(root)) {
      s = s.slice(root.length);
      break;
    }
  }
  return s.replace(/^\.\//, "");
}

/** Normalize a backticked checklist path to comparable segments. */
function normCheckPath(p) {
  let s = String(p).trim().toLowerCase().replace(/^\.\//, "");
  let prefix = false;
  if (s.endsWith("/**")) {
    prefix = true;
    s = s.slice(0, -3);
  } else if (s.endsWith("**")) {
    prefix = true;
    s = s.slice(0, -2);
  }
  // Checklists sometimes quote repo-rooted paths; reduce to the same basis.
  s = areaPath(s);
  // Leading ellipses (`…/projects/…`, `.../x`) stand for elided prefixes.
  s = s.replace(/^([….]|…|\.\.\.)\/+/, "");
  const segs = s.split("/").filter(Boolean);
  return { segs, prefix };
}

/**
 * Segment match of a checklist path against an item path, two ways:
 *  - area-prefix: checklist is a leading run of the area-basis item path
 *    (`projects/…/archives/**` covers everything beneath it);
 *  - suffix: checklist is a trailing run of the raw item path
 *    (`(home)/page.tsx` names the file under its parent group).
 * `*` matches one segment. Single-solid-segment paths never match (too generic).
 */
function segsEqual(a, b) {
  if (a === "*" || b === "*") return true;
  return a === b;
}

function pathCovers(check, areaSegs, rawSegs) {
  const { segs } = check;
  const isGap = (g) => g === "*" || g === "**" || g === "..." || g === "…";
  const solid = segs.filter((g) => !isGap(g)).length;
  if (!segs.length) return false;
  const clean = segs.filter((g) => g !== "**");
  if (solid >= 2) {
    if (clean.length <= areaSegs.length && clean.every((g, i) => segsEqual(g, areaSegs[i]))) return true;
    if (clean.length <= rawSegs.length) {
      const tail = rawSegs.slice(rawSegs.length - clean.length);
      if (clean.every((g, i) => segsEqual(g, tail[i]))) return true;
    }
    // Gap runs (`.../file-assets/.../restore/`): solid runs appear in order.
    if (segs.some(isGap)) {
      const runs = [];
      let cur = [];
      for (const g of segs.map((s) => String(s).toLowerCase())) {
        if (isGap(g)) {
          if (cur.length) runs.push(cur);
          cur = [];
        } else cur.push(g);
      }
      if (cur.length) runs.push(cur);
      for (const hay of [areaSegs, rawSegs]) {
        const low = hay.map((s) => String(s).toLowerCase());
        let from = 0;
        let ok = true;
        for (const run of runs) {
          let found = -1;
          for (let i = from; i + run.length <= low.length; i++) {
            if (run.every((g, j) => segsEqual(g, low[i + j]))) {
              found = i + run.length;
              break;
            }
          }
          if (found < 0) {
            ok = false;
            break;
          }
          from = found;
        }
        if (ok) return true;
      }
    }
    return false;
  }
  // Single-solid-segment claims only count as explicit directory claims
  // (`dropdowns/`): the name must appear as a whole path segment run, and
  // structural names (`app/`, `core/`) never count.
  if (!String(check.raw).endsWith("/")) return false;
  const want = clean[0].toLowerCase();
  if (GENERIC_DIRS.has(want)) return false;
  const hay = [...areaSegs, ...rawSegs].map((s) => String(s).toLowerCase());
  return hay.some((s) => s === want || s === `${want}s` || `${s}s` === want);
}

/**
 * Normalize a backend endpoint for comparison: drop the HTTP verb, lowercase,
 * unify every placeholder (`${x}`, `{slug}`, `:anchor`) to `{}`.
 */
function normEndpoint(s) {
  const segs = String(s)
    .toLowerCase()
    .replace(/`\s*\+.*$/, "")
    .replace(/\?.*$/, "")
    .replace(/[[\]]/g, "")
    .split("/")
    .map((g) => g.trim().replace(/^[.\s|]+/, "").replace(/^(get|post|put|patch|delete|head|options)\s+/i, "").trim())
    .filter((g) => g && g !== "..." && !/^(get|post|put|patch|delete|head|options)$/.test(g));
  // Drop leading inventory shorthands (BASE/, WS/, .../).
  while (segs.length && /^(base|ws|\.\.\.)$/i.test(segs[0])) segs.shift();
  return segs
    .join("/")
    .replace(/\$\{[^}]*\}/g, "{}")
    .replace(/\{[^}]*\}/g, "{}")
    .replace(/:[a-z_][a-z0-9_]*/g, "{}")
    .replace(/\/+$/, "")
    .trim();
}

/** Solid (non-placeholder) segments of a normalized endpoint. */
function endpointSolids(norm) {
  return norm.split("/").filter((g) => g && g !== "{}");
}

/** App scope of an item id (oss admin/space/web/desktop vs cloud overlay). */
function itemScope(id) {
  const s = String(id);
  if (s.includes("ee-overlay")) return "ee-overlay";
  if (s.startsWith("apps/admin/")) return "apps/admin";
  if (s.startsWith("apps/space/")) return "apps/space";
  if (s.startsWith("apps/web/")) return "apps/web";
  if (s.startsWith("desktop-overlay/")) return "desktop-overlay";
  if (s.startsWith("packages/")) return "packages";
  return null;
}

function parseInventory(path) {
  const text = read(path) || "";
  const rows = new Set();
  for (const m of text.matchAll(/^\|\s*([A-Z][A-Z0-9]*-\d+)\s*\|/gm)) rows.add(m[1]);
  const checklistCells = [];
  const explained = [];
  const checkPaths = [];
  const explainedPaths = [];
  const lines = text.split("\n");
  let inChecklist = false;
  let inExplained = false;
  let scope = null; // most recent app marker; checklist paths inherit it
  let sectionBase = null; // dir from the section heading cells are relative to
  const scopeOf = (line) => {
    const m = /apps\/(admin|space|web)\b|desktop-overlay|ee-overlay|private-pi-dash|packages\/services/.exec(line);
    if (!m) return null;
    if (m[0] === "desktop-overlay") return "desktop-overlay";
    if (m[0] === "ee-overlay" || m[0] === "private-pi-dash") return "ee-overlay";
    if (m[0] === "packages/services") return "packages";
    return `apps/${m[1]}`;
  };
  // A section heading may declare the base its cells are relative to:
  // "### Top-level component folders (`core/components/stickies`)".
  const dirBaseOf = (line) => {
    for (const m of line.matchAll(/`([^`]{3,160})`/g)) {
      let cand = m[1].trim();
      if (!cand.includes("/") || cand.includes("*")) continue;
      cand = cand.replace(/\/\*\*$/, "").replace(/\/$/, "");
      if (/\.(tsx?|css|ts|json)$/.test(cand)) cand = cand.slice(0, cand.lastIndexOf("/"));
      if (cand && !/^(get|post|put|patch|delete)\s/i.test(cand) && !cand.startsWith("/api/")) return cand;
    }
    return null;
  };
  const harvest = (line, via) => {
    const spans = [...line.matchAll(/`([^`]{2,160})`/g)].map((m) => m[1].trim());
    // `dir/page.tsx` + `layout.tsx` pairs: expand the bare sibling filenames
    // against the slashed path's directory.
    const slashed = spans.filter((c) => c.includes("/"));
    const expanded = [...spans];
    for (const s of spans) {
      if (!s.includes("/") && /\.(tsx?|css)$/.test(s) && slashed.length) {
        const dir = slashed[0].slice(0, slashed[0].lastIndexOf("/"));
        expanded.push(`${dir}/${s}`);
      }
    }
    for (const cand of expanded) {
      if (/[\\/]/.test(cand) || /\.(tsx?|css)$/.test(cand) || cand.endsWith("**")) {
        checkPaths.push({ raw: cand, scope, via, ...normCheckPath(cand) });
        // Plus the section-base-resolved variant: cells are often relative to
        // the heading's base (`layout/x` under `core/components/stickies`).
        // Doubled variants (cells already rooted) match no real file and are
        // harmless; the correctly resolved ones close the gap.
        if (sectionBase) {
          const first = cand.split("/")[0].toLowerCase();
          const baseFirst = sectionBase.split("/")[0].toLowerCase();
          if (first && first !== baseFirst) {
            const resolved = `${sectionBase}/${cand.replace(/^\.\//, "")}`;
            checkPaths.push({ raw: `${resolved} (via ${sectionBase})`, scope, via, ...normCheckPath(resolved) });
          }
        }
      }
    }
  };
  // Prose shorthand (`app/`, `the page`) is not a disownable path: explained
  // harvesting requires at least two segments.
  const harvestExplained = (line) => {
    for (const m of line.matchAll(/`([^`]{2,160})`/g)) {
      const cand = m[1].trim();
      if (!(/[\\/]/.test(cand) || /\.(tsx?|css)$/.test(cand))) continue;
      const norm = normCheckPath(cand);
      if (norm.segs.length < 2) continue;
      explainedPaths.push({ raw: cand, scope, ...norm });
    }
  };
  const DISOWN = /out of scope|do not belong|no row|not counted|dead code|not rowed/i;
  let para = [];
  const flushPara = () => {
    if (inChecklist && para.length) {
      const text = para.join("\n");
      if (DISOWN.test(text)) harvestExplained(text);
    }
    para = [];
  };
  for (const line of lines) {
    const hm = /^(#{1,4})\s/.exec(line);
    if (hm) {
      flushPara();
      const level = hm[1].length;
      const isCheck = /checklist|coverage|\bin scope\b/i.test(line);
      const isExpl = /explain|no.?row|out of scope|not rowed|dead code|sweeps with no/i.test(line);
      if (level <= 2) {
        // New top-level section: previous modes end unless re-declared.
        // Explained wins ties ("Explicitly out of scope" style sections).
        inExplained = isExpl;
        inChecklist = isCheck && !isExpl;
      } else if (isExpl) {
        inExplained = true;
        inChecklist = false;
      } else if (isCheck) {
        inChecklist = true;
        inExplained = false;
      }
      // Any other ###+ heading continues the enclosing ## section's mode.
      scope = scopeOf(line) || scope;
      const base = dirBaseOf(line);
      if (level <= 2) sectionBase = base;
      else if (base) sectionBase = base;
    } else if (inChecklist && (/^\|/.test(line) && !/^\|\s*-/.test(line) || /^\s*-\s+/.test(line))) {
      flushPara();
      checklistCells.push(line);
      // A line that names rows claims its paths; a line that disowns them
      // ("dead code, no row") explains them; anything else claims.
      // NEWFRONT-nnn references are issue links, not row claims.
      const rowRefs = line.match(/[A-Z][A-Z0-9]*-\d+/g) || [];
      if (rowRefs.some((r) => !r.startsWith("NEWFRONT-"))) harvest(line, "checklist");
      else if (DISOWN.test(line)) harvestExplained(line);
      else harvest(line, "checklist");
    } else if (/^\|\s*[A-Z][A-Z0-9]*-\d+\s*\|/.test(line)) {
      // Row-table entry ("Old entry point" cells name sources too).
      flushPara();
      harvest(line, "row");
    } else if (inExplained) {
      flushPara();
      explained.push(line);
    } else if (/^\s*$/.test(line)) {
      flushPara();
    } else if (/:\s*$/.test(line) && /`[^`]*apps\/(admin|space|web)\b[^`]*`|ee-overlay|desktop-overlay/.test(line)) {
      // Section-intro paragraph ("Route files (`apps/space/app/**`):").
      // Row tables and bullets never change scope: edition asides inside row
      // text must not re-scope the checklist paths that follow.
      flushPara();
      scope = scopeOf(line) || scope;
    } else {
      // Ordinary prose: buffer the paragraph; a disown phrase anywhere in the
      // block disowns every path named in it.
      para.push(line);
    }
  }
  flushPara();
  const fold = (s) => String(s).toLowerCase().replace(/[^a-z0-9]/g, "");
  return {
    file: path,
    rows,
    checklist: checklistCells.join("\n").toLowerCase(),
    checkPaths,
    explainedPaths,
    explained: explained.join("\n").toLowerCase(),
    full: text.toLowerCase(),
    folded: fold(text),
  };
}

/* ------------------------------------------------------------------ */
/* matching                                                            */
/* ------------------------------------------------------------------ */

/** A match key is usable when long/distinctive enough to avoid noise. */
function usableKey(k) {
  const s = String(k).toLowerCase();
  if (s.length < 4) return false;
  if (/^[a-z]+$/.test(s) && s.length < 8) return false; // bare words: list, get…
  return true;
}

function matchItem(item, inventories) {
  const covered = [];
  const explained = [];
  const isPath =
    item.category === "routes" ||
    item.category === "components" ||
    item.category === "redirects-errors" ||
    item.category === "shortcuts";
  const scope = itemScope(item.id);
  // Step 0 (paths only): checklist segment match, scope-guarded.
  if (isPath) {
    const areaSegs = areaPath(item.id).toLowerCase().split("/").filter(Boolean);
    const rawSegs = String(item.id).toLowerCase().split("/").filter(Boolean);
    const isDir = item.kind === "component-dir";
    for (const inv of inventories) {
      const hit = inv.checkPaths.find((c) => {
        if (c.scope && c.scope !== scope) return false;
        if (pathCovers(c, areaSegs, rawSegs)) return true;
        // A component folder counts as inventoried when the checklist names
        // files inside it (the auditor walked it and rowed its content).
        if (!isDir) return false;
        const clean = (c.segs || []).filter((g) => g !== "**");
        if (!clean.length || GENERIC_DIRS.has(clean[clean.length - 1])) return false;
        const startsWith = (hay, needle) =>
          needle.length < hay.length && needle.every((g, i) => segsEqual(g, hay[i]));
        return startsWith(clean, areaSegs) || startsWith(clean, rawSegs);
      });
      if (hit) covered.push({ file: inv.file, evidence: `${hit.via || "checklist"} \`${hit.raw}\` covers`, checklist: true });
    }
    if (covered.length) return { status: "covered", matches: covered };
  }
  // Step 0b (service methods): normalized endpoint comparison against the
  // endpoint tables harvested as checklist paths. Backend endpoints are shared
  // across apps, so scope is deliberately ignored here.
  if (item.category === "service-methods") {
    const eps = (item.keys || []).filter((k) => String(k).includes("/")).map(normEndpoint).filter(Boolean);
    const segEq = (a, b) => a === "{}" || b === "{}" || a === b;
    const epMatch = (paths, minOverlap) =>
      paths.find((c) => {
        if (!String(c.raw).includes("/")) return false;
        const n = normEndpoint(c.raw);
        if (!endpointSolids(n).length) return false;
        return eps.some((e) => {
          const A = endpointSolids(e);
          const B = endpointSolids(n);
          const [shorter, longer] = A.length <= B.length ? [A, B] : [B, A];
          if (shorter.length < minOverlap) return false;
          if (shorter.length === 1 && shorter[0].length < 4) return false;
          const tail = longer.slice(longer.length - shorter.length);
          return shorter.every((g, i) => segEq(g, tail[i]));
        });
      });
    for (const inv of inventories) {
      const hit = epMatch(inv.checkPaths, 1);
      if (hit) covered.push({ file: inv.file, evidence: `endpoint \`${hit.raw}\` matches`, checklist: true });
    }
    if (covered.length) return { status: "covered", matches: covered };
    // Endpoints named in dead-code/no-row notes explain the method instead.
    // Single-fragment notes (`.../restore/`) never disown on their own, and a
    // note naming one shape (`GET .../pods/{id}/`) never disowns another
    // (`GET .../pods/`): explained matches need equal segment counts.
    for (const inv of inventories) {
      const hit = (inv.explainedPaths || []).find((c) => {
        if (!String(c.raw).includes("/")) return false;
        const n = normEndpoint(c.raw);
        const cn = n.split("/").filter(Boolean);
        if (!endpointSolids(n).length) return false;
        return eps.some((e) => {
          const en = e.split("/").filter(Boolean);
          if (en.length !== cn.length) return false;
          const A = endpointSolids(e);
          const B = endpointSolids(n);
          const [shorter, longer] = A.length <= B.length ? [A, B] : [B, A];
          if (shorter.length < 2) return false;
          const tail = longer.slice(longer.length - shorter.length);
          return shorter.every((g, i) => segEq(g, tail[i]));
        });
      });
      if (hit) explained.push({ file: inv.file, evidence: `endpoint \`${hit.raw}\` disowned` });
    }
    if (explained.length) return { status: "explained", matches: explained };
  }
  // Step 0c (paths): explicitly disowned checklist prose ("out of scope…").
  if (isPath) {
    const areaSegs = areaPath(item.id).toLowerCase().split("/").filter(Boolean);
    const rawSegs = String(item.id).toLowerCase().split("/").filter(Boolean);
    for (const inv of inventories) {
      const hit = (inv.explainedPaths || []).find(
        (c) => (!c.scope || c.scope === scope) && pathCovers(c, areaSegs, rawSegs)
      );
      if (hit) explained.push({ file: inv.file, evidence: `disowned \`${hit.raw}\`` });
    }
    if (explained.length) return { status: "explained", matches: explained };
  }
  // Folded operation-name match (service methods only): `listDevMachines`
  // claims prose like "list dev machines (by workspace)" in a row's API cell.
  // Names fold to fewer than 8 alphanumerics are too generic to use this way.
  if (item.category === "service-methods") {
    for (const inv of inventories) {
      const hit = (item.keys || []).find((k) => {
        const f = String(k).toLowerCase().replace(/[^a-z0-9]/g, "");
        if (f.length < 8 || f.includes("/")) return false;
        if (f === String(k).toLowerCase() && !/[A-Z]/.test(String(k).slice(1))) return false;
        return inv.folded.includes(f);
      });
      if (hit) covered.push({ file: inv.file, evidence: `operation \`${hit}\` named`, checklist: false });
    }
    if (covered.length) return { status: "covered", matches: covered };
  }
  for (const inv of inventories) {
    const hits = (item.keys || []).filter(usableKey).filter((k) => inv.full.includes(String(k).toLowerCase()));
    if (!hits.length) continue;
    const key = String(hits[0]).toLowerCase();
    const inExplained = inv.explained.includes(key);
    const inChecklist = inv.checklist.includes(key);
    if (inExplained && !inChecklist) explained.push({ file: inv.file, evidence: hits[0] });
    else covered.push({ file: inv.file, evidence: hits[0], checklist: inChecklist });
  }
  if (covered.length) return { status: "covered", matches: covered };
  if (explained.length) return { status: "explained", matches: explained };
  return { status: "unclaimed", matches: [] };
}

/* ------------------------------------------------------------------ */
/* report                                                              */
/* ------------------------------------------------------------------ */

function shortFile(root, absPath) {
  const r = relative(root, absPath).split("\\").join("/");
  return r.startsWith("..") ? absPath : r;
}

function renderSection(title, items, root, showEvidence) {
  let s = `## ${title} (${items.length})\n\n`;
  if (!items.length) {
    s += "_none_\n\n";
    return s;
  }
  s += "| Item | Inventory evidence |\n|------|--------------------|\n";
  for (const it of items) {
    const ev = showEvidence
      ? it.result.matches.map((m) => `\`${shortFile(root, m.file)}\` via \`${m.evidence}\``).join("<br>")
      : "—";
    s += `| \`${it.id}\` | ${ev} |\n`;
  }
  return s + "\n";
}

function renderReport(root, out, categories, missing, cloudDir, cloudPresent, counts) {
  const date = new Date().toISOString().slice(0, 10);
  let s = `# Coverage audit — old frontends vs feature inventories\n\n`;
  s += `Generated ${date} by \`apps/web_new/e2e/parity/tools/coverage-audit.mjs\` (rerunnable; report only).\n\n`;
  s += `Sources enumerated: \`apps/web/app\`, \`apps/admin/app\`, \`apps/space/app\`,\n`;
  s += `\`desktop-overlay/apps/web/app\`, component trees under \`core/components\` /\n`;
  s += `\`ce/components\` (+ admin/space/desktop equivalents), service classes under\n`;
  s += `\`apps/web/core/services\`, \`packages/services/src\` (+ admin/space/cloud services),\n`;
  s += `palette/shortcut registrations, redirects and error routes. Cloud overlay:\n`;
  s += cloudPresent ? `\`${cloudDir}\` (present).\n` : `unavailable (${cloudDir} not present — cloud section skipped, not failed).\n`;
  if (missing.length) {
    s += `\nSource directories absent from this checkout (not audited, not failures):\n`;
    for (const m of missing) s += `- \`${m}\`\n`;
  }
  s += `\n### Totals\n\n| Category | Items | Covered | Explained | Unclaimed |\n`;
  s += `|----------|-------|---------|-----------|-----------|\n`;
  for (const c of categories) {
    s += `| ${c.name} | ${c.items.length} | ${c.covered} | ${c.explained} | ${c.unclaimed} |\n`;
  }
  s += `| **All** | **${counts.items}** | **${counts.covered}** | **${counts.explained}** | **${counts.unclaimed}** |\n\n`;
  s += `Matching is layered per item: coverage-checklist path mention, then any\n`;
  s += `row/checklist text mention (with the matched string quoted as evidence), then\n`;
  s += `explained/no-row section mention. Anything left is **unclaimed** and must gain\n`;
  s += `rows (or an explained entry) in its owning area file before sign-off.\n\n`;
  for (const c of categories) {
    const cov = c.results.filter((r) => r.result.status === "covered");
    const exp = c.results.filter((r) => r.result.status === "explained");
    const unc = c.results.filter((r) => r.result.status === "unclaimed");
    s += `---\n\n### Category: ${c.name}\n\n`;
    s += renderSection("Covered", cov, root, true);
    s += renderSection("Explained (no row, reason in inventory)", exp, root, true);
    s += renderSection("Unclaimed", unc, root, false);
  }
  writeFileSync(out, s);
}

/* ------------------------------------------------------------------ */
/* main                                                                */
/* ------------------------------------------------------------------ */

function parseArgs(argv) {
  const args = {
    root: DEFAULT_ROOT,
    cloud: process.env.CLOUD_OVERLAY_DIR || "/tmp/audit-sync/cloud",
    out: null,
    strict: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--root") args.root = resolve(argv[++i]);
    else if (a === "--cloud-overlay") args.cloud = argv[++i];
    else if (a === "--out") args.out = resolve(argv[++i]);
    else if (a === "--strict") args.strict = true;
    else if (a === "--help" || a === "-h") args.help = true;
    else throw new Error(`unknown argument: ${a}`);
  }
  if (!args.out) args.out = join(args.root, DEFAULT_OUT);
  return args;
}

function main(argv) {
  const args = parseArgs(argv);
  if (args.help) {
    console.log(
      "coverage-audit.mjs: cross-check old frontends against feature inventories.\n" +
        "Options: --root <dir> --cloud-overlay <dir> --out <file> --strict --help"
    );
    return 0;
  }
  const root = args.root;
  const cloudPresent = existsSync(join(args.cloud, "ee-overlay"));
  const relCloud = (p) => `${args.cloud}/${p}`;

  const APP_DIRS = ["apps/web/app", "apps/admin/app", "apps/space/app", "desktop-overlay/apps/web/app"];
  if (cloudPresent) APP_DIRS.push(relCloud("ee-overlay/apps/web/app"), relCloud("ee-overlay/apps/admin/app"));

  const COMP_DIRS = [
    "apps/web/core/components",
    "apps/web/ce/components",
    "apps/admin/components",
    "apps/space/components",
    "desktop-overlay/apps/web/core/components",
  ];
  if (cloudPresent) {
    COMP_DIRS.push(
      relCloud("ee-overlay/apps/web/core/components"),
      relCloud("ee-overlay/apps/web/ce/components"),
      relCloud("ee-overlay/apps/web/ee/components")
    );
  }

  const SERVICE_DIRS = ["apps/web/core/services", "packages/services/src", "apps/admin/lib", "apps/space/lib"];
  if (cloudPresent) SERVICE_DIRS.push(relCloud("ee-overlay/apps/web/core/services"));

  // Enumerators take repo-relative dirs; absolutize the /tmp cloud paths.
  const abs_ = (d) => (d.startsWith("/") ? d : join(root, d));
  const routes = enumerateRoutesRaw(root, APP_DIRS.map(abs_));
  const components = enumerateComponentsRaw(root, COMP_DIRS.map(abs_));
  const services = enumerateServicesRaw(root, SERVICE_DIRS.map(abs_));
  const shortcuts = enumerateShortcuts(root, cloudPresent ? args.cloud : null);
  const redir = enumerateRedirects(root, cloudPresent ? args.cloud : null);

  const invDir = join(root, ".ai_design", "new_frontend_implementation", "parity", "inventory");
  const invFiles = existsSync(invDir)
    ? readdirSync(invDir).filter((f) => f.endsWith(".md")).map((f) => join(invDir, f))
    : [];
  if (!invFiles.length) throw new Error(`no inventory files in ${invDir}`);
  const inventories = invFiles.map(parseInventory);

  const byId = (arr) => {
    const seen = new Set();
    return arr.filter((i) => {
      if (seen.has(i.id)) return false;
      seen.add(i.id);
      return true;
    });
  };
  const categories = [
    { name: "routes", items: routes.filter((i) => !i.missing && i.category === "routes") },
    { name: "components", items: components.filter((i) => !i.missing) },
    { name: "service-methods", items: services.filter((i) => !i.missing) },
    { name: "shortcuts", items: shortcuts.filter((i) => !i.missing) },
    {
      name: "redirects-errors",
      items: byId(
        [...routes, ...redir].filter((i) => !i.missing && i.category === "redirects-errors")
      ),
    },
  ];
  const missing = [
    ...routes.filter((i) => i.missing).map((i) => i.source),
    ...components.filter((i) => i.missing).map((i) => i.source),
    ...services.filter((i) => i.missing).map((i) => i.source),
  ];
  if (!cloudPresent) missing.push(`${args.cloud}/ee-overlay (cloud overlay)`);

  const counts = { items: 0, covered: 0, explained: 0, unclaimed: 0 };
  for (const c of categories) {
    c.results = c.items.map((item) => ({ id: item.id, result: matchItem(item, inventories) }));
    c.covered = c.results.filter((r) => r.result.status === "covered").length;
    c.explained = c.results.filter((r) => r.result.status === "explained").length;
    c.unclaimed = c.results.filter((r) => r.result.status === "unclaimed").length;
    counts.items += c.items.length;
    counts.covered += c.covered;
    counts.explained += c.explained;
    counts.unclaimed += c.unclaimed;
  }

  renderReport(root, args.out, categories, [...new Set(missing)], args.cloud, cloudPresent, counts);
  console.log(`items=${counts.items} covered=${counts.covered} explained=${counts.explained} unclaimed=${counts.unclaimed}`);
  console.log(`report: ${relative(root, args.out).split("\\").join("/")}`);
  if (counts.unclaimed) {
    console.log("unclaimed items:");
    for (const c of categories)
      for (const r of c.results.filter((x) => x.result.status === "unclaimed")) console.log(`  [${c.name}] ${r.id}`);
  }
  if (args.strict && counts.unclaimed) return 1;
  return 0;
}

// Same enumerators, but accepting absolute directories (cloud lives outside root).
function enumerateRoutesRaw(root, absDirs) {
  const out = [];
  for (const abs of absDirs) {
    if (!existsSync(abs)) {
      out.push({ missing: true, source: abs });
      continue;
    }
    const inRoot = !relative(root, abs).startsWith("..");
    for (const f of walk(abs)) {
      const r = inRoot ? rel(root, f) : abs + "/" + relative(abs, f).split("\\").join("/");
      if (!r) continue;
      const base = f.split("/").pop();
      if (ROUTE_FILES.has(base)) {
        const kind = base === "not-found.tsx" || base === "global-error.tsx" || base === "error.tsx"
          ? "error-route"
          : "route-file";
        out.push({
          category: kind === "error-route" ? "redirects-errors" : "routes",
          kind,
          id: r,
          keys: [r, stripGroups(r)],
        });
      }
      if (base === "routes.ts" && /\/app\/routes\.ts$/.test(r)) {
        const text = read(f) || "";
        const re = /(?:route|index|layout)\(\s*"([^"]+)"\s*,\s*"([^"]+)"/g;
        let m;
        while ((m = re.exec(text))) {
          const target = m[2].replace(/^\.\//, "");
          const keys = [m[2], target, stripGroups(target), m[1]];
          const slash = target.lastIndexOf("/");
          if (slash >= 0) keys.push(target.slice(slash + 1));
          out.push({
            category: "routes",
            kind: "route-config",
            id: `${r} :: ${m[1]} -> ${m[2]}`,
            keys,
          });
        }
      }
    }
  }
  return out;
}

function enumerateComponentsRaw(root, absDirs) {
  const out = [];
  for (const abs of absDirs) {
    if (!existsSync(abs)) {
      out.push({ missing: true, source: abs });
      continue;
    }
    const inRoot = !relative(root, abs).startsWith("..");
    const label = (d) => (inRoot ? rel(root, d) : abs + "/" + relative(abs, d).split("\\").join("/"));
    let level1;
    try {
      level1 = readdirSync(abs, { withFileTypes: true }).filter((e) => e.isDirectory());
    } catch {
      continue;
    }
    for (const l1 of level1) {
      const d1 = join(abs, l1.name);
      const r1 = label(d1);
      if (r1) out.push({ category: "components", kind: "component-dir", id: r1, keys: [r1, areaPath(r1)] });
      let level2;
      try {
        level2 = readdirSync(d1, { withFileTypes: true }).filter((e) => e.isDirectory());
      } catch {
        continue;
      }
      for (const l2 of level2) {
        const r2 = label(join(d1, l2.name));
        if (r2) out.push({ category: "components", kind: "component-dir", id: r2, keys: [r2, areaPath(r2)] });
      }
    }
  }
  return out;
}

function enumerateServicesRaw(root, absDirs) {
  const files = [];
  const out = [];
  for (const abs of absDirs) {
    if (!existsSync(abs)) {
      out.push({ missing: true, source: abs });
      continue;
    }
    files.push(...walk(abs).filter((f) => /\.service\.ts$/.test(f) || /services?\/.*\.ts$/.test(f)));
  }
  return out.concat(enumerateServicesFromFiles(root, files));
}

process.exit(main(process.argv.slice(2)));