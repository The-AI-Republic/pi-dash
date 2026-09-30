// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Coexistence routing (F-09/NEWFRONT-20). The proxy serves listed prefixes
// from web_new and everything else from the old app, on one origin (so the
// session cookie is shared — no origin or cookie change is involved).
// The single source of truth is apps/web_new/migrated-routes.json.
// Usage: `pnpm --filter web_new coexistence:generate` rewrites the marked
// blocks in both Caddyfiles; `--check` (CI) fails when they are stale.
// Area gates append their prefixes to the JSON and re-run the generator.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(join(here, "..", "..", ".."));

const LIST = join(root, "apps", "web_new", "migrated-routes.json");
const CADDY_CE = join(root, "apps", "proxy", "Caddyfile.ce");
const CADDY_AIO = join(root, "apps", "proxy", "Caddyfile.aio.ce");

// Upstream name the web_new service will be known by (Docker convention,
// mirroring `web:3000`). Provisional until a deploy issue wires the service;
// the merged list is empty, so no Caddyfile references it until then.
const WEB_NEW_UPSTREAM = "web_new:3000";

const MARK_BEGIN =
  "BEGIN MIGRATED ROUTES (generated from apps/web_new/migrated-routes.json — do not edit; run `pnpm --filter web_new coexistence:generate`)";
const MARK_END = "END MIGRATED ROUTES";

const CE_ANCHOR = "\treverse_proxy /* web:3000";
const AIO_ANCHOR = "    handle_path /* {";

const EMPTY_NOTE = "(none — the list is empty, so every path routes to the old app)";

/** Read and validate the prefix list. Throws on the first problem. */
export function loadPrefixes(listPath = LIST) {
  const raw = JSON.parse(readFileSync(listPath, "utf8"));
  if (!Array.isArray(raw.prefixes)) throw new Error(`${listPath}: "prefixes" must be an array`);
  const seen = new Set();
  for (const prefix of raw.prefixes) {
    if (typeof prefix !== "string" || !prefix.startsWith("/")) {
      throw new Error(`${listPath}: each prefix must be a leading-slash path, got ${JSON.stringify(prefix)}`);
    }
    if (/[\s?#]/.test(prefix)) throw new Error(`${listPath}: prefix must not contain spaces, ? or #: ${prefix}`);
    if (prefix !== "/" && prefix.endsWith("/")) {
      throw new Error(`${listPath}: prefix must not end with a slash (except "/"): ${prefix}`);
    }
    if (seen.has(prefix)) throw new Error(`${listPath}: duplicate prefix: ${prefix}`);
    seen.add(prefix);
  }
  return [...seen];
}

/** Route lines for the CE Caddyfile (tab-indented, matcher style). */
export function buildCeLines(prefixes) {
  if (prefixes.length === 0) return [`\t# ${EMPTY_NOTE}`];
  const lines = [];
  for (const prefix of prefixes) {
    if (prefix === "/") {
      // Earlier identical matcher wins, so this shadows the old catch-all below.
      lines.push(`\treverse_proxy /* ${WEB_NEW_UPSTREAM}`);
      continue;
    }
    lines.push(`\tredir ${prefix} ${prefix}/ permanent`);
    lines.push(`\treverse_proxy ${prefix}/* ${WEB_NEW_UPSTREAM}`);
  }
  return lines;
}

/** Route lines for the AIO Caddyfile (4-space indent, handle style). */
export function buildAioLines(prefixes) {
  if (prefixes.length === 0) return [`    # ${EMPTY_NOTE}`];
  const lines = [];
  for (const prefix of prefixes) {
    if (prefix === "/") {
      lines.push(`    handle_path /* {`);
      lines.push(`        root * /app/web_new`);
      lines.push(`        try_files {path} {path}/ /index.html`);
      lines.push(`        file_server`);
      lines.push(`    }`);
      continue;
    }
    // Same shape as the sibling god-mode/web blocks: the AIO image serves
    // frontends as static files, so web_new lands under /app/web_new.
    lines.push(`    handle_path ${prefix}* {`);
    lines.push(`        root * /app/web_new`);
    lines.push(`        try_files {path} {path}/ /index.html`);
    lines.push(`        file_server`);
    lines.push(`    }`);
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
  const prefixes = loadPrefixes(opts.list);
  const ce = syncFile(opts.ce, CE_ANCHOR, "\t", buildCeLines(prefixes), { check: opts.check });
  const aio = syncFile(opts.aio, AIO_ANCHOR, "    ", buildAioLines(prefixes), { check: opts.check });
  if (opts.check) {
    console.log(`Coexistence check passed (${prefixes.length} migrated prefix(es)).`);
  } else {
    console.log(`Coexistence routes: CE ${ce}, AIO ${aio} (${prefixes.length} migrated prefix(es)).`);
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
