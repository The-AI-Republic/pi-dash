// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Coexistence routing (F-09/NEWFRONT-20, workspace-scoped form NEWFRONT-528).
// The proxy serves listed routes from web_new and everything else from the
// old app, on one origin (so the session cookie is shared — no origin or
// cookie change is involved). The single source of truth is
// apps/web_new/migrated-routes.json.
//
// Entry grammar (one entry per route):
//   /a/b           literal path; serves it and its subtree (unchanged)
//   /{ws}          one variable segment, exactly that depth (a workspace home)
//   /{ws}/projects one variable segment then literals, exact depth
//   /{ws}/x/*      the same, plus every path below it
// A variable first segment is excluded from `reservedTopLevel` entries
// (case-insensitive) and from every literal entry's first segment: those
// top-level paths belong to the old app, the admin console, public boards or
// the backend, never to a workspace. A variable segment also never matches a
// segment containing a dot, since workspace slugs cannot contain one. The
// app's isMigratedPath mirrors these rules exactly; the shared fixtures in
// this directory's tests and shared/coexistence/migrated-routes.test.ts keep
// the two sides honest.
//
// Usage: `pnpm --filter web_new coexistence:generate` rewrites the marked
// blocks in both Caddyfiles; `--check` (CI) fails when they are stale.
// Area gates append their entries to the JSON and re-run the generator.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(join(here, "..", "..", ".."));

const LIST = join(root, "apps", "web_new", "migrated-routes.json");
const CADDY_CE = join(root, "apps", "proxy", "Caddyfile.ce");
const CADDY_AIO = join(root, "apps", "proxy", "Caddyfile.aio.ce");

// Upstream name the web_new service will be known by (Docker convention,
// mirroring `web:3000`).
const WEB_NEW_UPSTREAM = "web_new:3000";

const MARK_BEGIN =
  "BEGIN MIGRATED ROUTES (generated from apps/web_new/migrated-routes.json — do not edit; run `pnpm --filter web_new coexistence:generate`)";
const MARK_END = "END MIGRATED ROUTES";

const CE_ANCHOR = "\treverse_proxy /* web:3000";
const AIO_ANCHOR = "    handle_path /* {";

const EMPTY_NOTE = "(none — the list is empty, so every path routes to the old app)";

const VARIABLE_SEGMENT = /^\{([A-Za-z][A-Za-z0-9]*)\}$/;
const RESERVED_ENTRY = /^[a-z0-9][a-z0-9-]*$/;

/**
 * Split an entry (already known to start with "/") into segments.
 * Returns null when the entry is the catch-all "/".
 */
export function parseEntrySegments(entry) {
  if (entry === "/") return null;
  return entry.slice(1).split("/");
}

/** One segment as a matcher part: literal, variable, or (last only) wildcard. */
export function parseSegment(segment, entry) {
  if (segment === "*") return { kind: "wildcard" };
  const variable = VARIABLE_SEGMENT.exec(segment);
  if (variable !== null) return { kind: "variable", name: variable[1] };
  if (segment === "" || segment.includes("{") || segment.includes("}") || segment.includes("*")) {
    throw new Error(`bad segment ${JSON.stringify(segment)} in entry ${entry}`);
  }
  return { kind: "literal", value: segment };
}

/** Parsed form of one entry, as both generators and the validator see it. */
export function parseEntry(entry) {
  const segments = parseEntrySegments(entry);
  if (segments === null) return { entry, catchAll: true, segments: [] };
  const parsed = segments.map((segment) => parseSegment(segment, entry));
  const wildcardAt = parsed.findIndex((segment) => segment.kind === "wildcard");
  if (wildcardAt !== -1 && wildcardAt !== parsed.length - 1) {
    throw new Error(`wildcard "*" must be the last segment: ${entry}`);
  }
  return { entry, catchAll: false, segments: parsed, subtree: wildcardAt !== -1 };
}

/** True when the entry's first segment is a variable (a workspace scope). */
export function isWorkspaceScoped(parsed) {
  return !parsed.catchAll && parsed.segments[0]?.kind === "variable";
}

/** The literal first segment of an entry, when it has one. */
function literalTopLevel(parsed) {
  const first = parsed.segments[0];
  return first?.kind === "literal" ? first.value : null;
}

/**
 * Top-level names a variable first segment must never match: the explicit
 * reserved list plus every literal entry's first segment, lowercased.
 */
export function reservedTopLevelOf(parsedEntries, reservedTopLevel = []) {
  const names = new Set(reservedTopLevel.map((name) => name.toLowerCase()));
  for (const parsed of parsedEntries) {
    const literal = literalTopLevel(parsed);
    if (literal !== null) names.add(literal.toLowerCase());
  }
  return [...names].sort();
}

/** Caddy path_regexp source for a non-catch-all entry. */
export function entryRegex(parsed) {
  const parts = ["^"];
  for (let index = 0; index < parsed.segments.length; index += 1) {
    const segment = parsed.segments[index];
    if (segment.kind === "wildcard") {
      // "/x/*" matches "/x" itself too: the subtree starts at the entry.
      parts.push("(?:/.*)?");
      break;
    }
    if (segment.kind === "variable") {
      // One non-empty segment with no slash and no dot (slug charset).
      parts.push("/[^/.]+");
    } else {
      parts.push(`/${segment.value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`);
    }
    if (index === parsed.segments.length - 1) parts.push("/?");
  }
  parts.push("$");
  return parts.join("");
}

/**
 * Routes to emit: the static auth entries (and any other literal entry)
 * keep their exact-match redirect plus subtree proxy; workspace-scoped
 * entries take named matchers, with the reserved exclusion when their first
 * segment is variable.
 */
function routableEntries(entries, reservedTopLevel) {
  const parsedEntries = entries.map(parseEntry);
  const reserved = reservedTopLevelOf(parsedEntries, reservedTopLevel);
  return parsedEntries.map((parsed, index) => ({
    entry: parsed.entry,
    parsed,
    name: `@web_new_${index}`,
    regex: parsed.catchAll ? null : entryRegex(parsed),
    excluded: isWorkspaceScoped(parsed) ? reserved : [],
  }));
}

/**
 * Validate the parsed list. Returns the entries untouched; throws on the
 * first problem. `reservedTopLevel` is checked for shape only — a reserved
 * name that also has a literal entry is not a conflict, the literal entry
 * is the explicit owner.
 */
export function validateList(raw, listPath = LIST) {
  if (!Array.isArray(raw.entries)) throw new Error(`${listPath}: "entries" must be an array`);
  const reserved = raw.reservedTopLevel ?? [];
  if (!Array.isArray(reserved)) throw new Error(`${listPath}: "reservedTopLevel" must be an array`);
  const reservedSeen = new Set();
  for (const name of reserved) {
    if (typeof name !== "string" || !RESERVED_ENTRY.test(name)) {
      throw new Error(`${listPath}: reservedTopLevel entries are lowercase slugs without a slash, got ${JSON.stringify(name)}`);
    }
    if (reservedSeen.has(name)) throw new Error(`${listPath}: duplicate reservedTopLevel entry: ${name}`);
    reservedSeen.add(name);
  }
  const seen = new Set();
  for (const entry of raw.entries) {
    if (typeof entry !== "string" || !entry.startsWith("/")) {
      throw new Error(`${listPath}: each entry must be a leading-slash route, got ${JSON.stringify(entry)}`);
    }
    if (/[\s?#]/.test(entry)) throw new Error(`${listPath}: entry must not contain spaces, ? or #: ${entry}`);
    if (entry !== "/" && (entry.endsWith("/") || entry.includes("//"))) {
      throw new Error(`${listPath}: entry must not end with, or repeat, a slash: ${entry}`);
    }
    if (entry !== "/" && entry.includes("{") !== entry.includes("}")) {
      throw new Error(`${listPath}: unbalanced variable braces: ${entry}`);
    }
    parseEntry(entry);
    if (seen.has(entry)) throw new Error(`${listPath}: duplicate entry: ${entry}`);
    seen.add(entry);
  }
  const catchAll = raw.entries.filter((entry) => entry === "/");
  if (catchAll.length > 0 && raw.entries.length > 1) {
    throw new Error(`${listPath}: "/" routes everything to web_new; remove the other entries or the catch-all`);
  }
  return [...seen];
}

/** Read and validate the entry list. Throws on the first problem. */
export function loadEntries(listPath = LIST) {
  const raw = JSON.parse(readFileSync(listPath, "utf8"));
  return { entries: validateList(raw, listPath), reservedTopLevel: raw.reservedTopLevel ?? [] };
}

function matcherBlock(route, indent) {
  const lines = [`${indent}${route.name} {`, `${indent}    path_regexp ${route.regex}`];
  if (route.excluded.length > 0) {
    // `path` is exact, so each name needs its subtree form too. The matcher
    // is case-insensitive, which is what the reserved check wants.
    lines.push(`${indent}    not path ${route.excluded.map((name) => `/${name} /${name}/*`).join(" ")}`);
  }
  lines.push(`${indent}}`);
  return lines;
}

/**
 * Route lines for the CE Caddyfile (tab-indented, matcher style). The block
 * sits above the old app's catch-all; earlier handlers win.
 */
export function buildCeLines(entries, reservedTopLevel = []) {
  if (entries.length === 0) return [`\t# ${EMPTY_NOTE}`];
  const lines = [];
  for (const route of routableEntries(entries, reservedTopLevel)) {
    if (route.parsed.catchAll) {
      lines.push(`\treverse_proxy /* ${WEB_NEW_UPSTREAM}`);
      continue;
    }
    if (!isWorkspaceScoped(route.parsed)) {
      lines.push(`\tredir ${route.entry} ${route.entry}/ permanent`);
      lines.push(`\treverse_proxy ${route.entry}/* ${WEB_NEW_UPSTREAM}`);
      continue;
    }
    lines.push(...matcherBlock(route, "\t"));
    lines.push(`\treverse_proxy ${route.name} ${WEB_NEW_UPSTREAM}`);
  }
  return lines;
}

/** Route lines for the AIO Caddyfile (4-space indent, handle style). */
export function buildAioLines(entries, reservedTopLevel = []) {
  if (entries.length === 0) return [`    # ${EMPTY_NOTE}`];
  const serve = (indent, matcher) => [
    `${indent}handle ${matcher} {`,
    `${indent}    root * /app/web_new`,
    `${indent}    try_files {path} {path}/ /index.html`,
    `${indent}    file_server`,
    `${indent}}`,
  ];
  const lines = [];
  for (const route of routableEntries(entries, reservedTopLevel)) {
    if (route.parsed.catchAll) {
      lines.push(...serve("    ", "*"));
      continue;
    }
    if (!isWorkspaceScoped(route.parsed)) {
      // Same shape as the sibling god-mode/web blocks: the AIO image serves
      // frontends as static files, so web_new lands under /app/web_new.
      lines.push(...serve("    ", `${route.entry}*`));
      continue;
    }
    lines.push(...matcherBlock(route, "    "));
    // `handle`, never `handle_path`: the matcher already scopes the route,
    // and stripping the workspace segment would break the SPA's own routes.
    lines.push(...serve("    ", route.name));
  }
  return lines;
}

function withMarkers(indent, routeLines) {
  return [`${indent}# ${MARK_BEGIN}`, ...routeLines, `${indent}# ${MARK_END}`];
}

/**
 * Bring one Caddyfile's marked block up to date. Returns "updated",
 * "unchanged", or throws when the anchor/markers are missing. In check
 * mode the file is never written; "updated" becomes the failure signal.
 */
export function syncFile(filePath, anchor, indent, routeLines, { check = false } = {}) {
  const text = readFileSync(filePath, "utf8");
  const lines = text.split("\n");
  const want = withMarkers(indent, routeLines);
  const begin = lines.findIndex((line) => line.trim() === `# ${MARK_BEGIN}`);
  const end = lines.findIndex((line) => line.trim() === `# ${MARK_END}`);
  if (begin !== -1 && end !== -1 && end > begin) {
    const current = lines.slice(begin, end + 1);
    if (current.join("\n") === want.join("\n")) return "unchanged";
    if (check) {
      throw new Error(
        `${filePath}: marked block is stale.\n--- want ---\n${want.join("\n")}\n--- have ---\n${current.join("\n")}`
      );
    }
    lines.splice(begin, end - begin + 1, ...want);
  } else if (begin === -1 && end === -1) {
    const at = lines.findIndex((line) => line === anchor);
    if (at === -1) throw new Error(`${filePath}: anchor line not found: ${JSON.stringify(anchor)}`);
    if (check) throw new Error(`${filePath}: marked block is missing (run the generator)`);
    lines.splice(at, 0, ...want);
  } else {
    throw new Error(`${filePath}: half-present marker block — repair it by hand, then re-run`);
  }
  writeFileSync(filePath, lines.join("\n"));
  return "updated";
}

function parseArgs(argv) {
  const opts = { check: false, list: LIST, ce: CADDY_CE, aio: CADDY_AIO };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--check") opts.check = true;
    else if (arg === "--list") opts.list = resolve(root, argv[(i += 1)]);
    else if (arg === "--caddy-ce") opts.ce = resolve(root, argv[(i += 1)]);
    else if (arg === "--caddy-aio") opts.aio = resolve(root, argv[(i += 1)]);
    else throw new Error(`unknown argument: ${arg}`);
  }
  return opts;
}

function main(argv) {
  const opts = parseArgs(argv);
  const { entries, reservedTopLevel } = loadEntries(opts.list);
  const ce = syncFile(opts.ce, CE_ANCHOR, "\t", buildCeLines(entries, reservedTopLevel), { check: opts.check });
  const aio = syncFile(opts.aio, AIO_ANCHOR, "    ", buildAioLines(entries, reservedTopLevel), {
    check: opts.check,
  });
  if (opts.check) {
    console.log(`Coexistence check passed (${entries.length} migrated route(s)).`);
  } else {
    console.log(`Coexistence routes: CE ${ce}, AIO ${aio} (${entries.length} migrated route(s)).`);
  }
}

const invokedAsScript = (process.argv[1] ?? "") === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`coexistence-routes: ${error instanceof Error ? error.message : error}`);
    process.exit(1);
  }
}
