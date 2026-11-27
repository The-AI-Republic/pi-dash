// Workflow branch-filter guard. The default branch is `main`; a workflow whose
// `on.<event>.branches` / `branches-ignore` filter names `master` never fires
// (or compares against a branch that does not exist). The workflow is parsed
// as YAML, so only real trigger filters count: quoted keys and multi-line flow
// lists are caught, and text inside `run:` scripts or `with:` inputs is not.
// Needs the pinned `yaml` package: npm ci --prefix .github/scripts
// Exits 1 and lists every offending file:line.
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { LineCounter, isMap, isScalar, isSeq, parseDocument } from "yaml";

const BANNED = ["master"];
const FILTER_KEYS = new Set(["branches", "branches-ignore"]);

const escapeRegExp = (s) => s.replace(/[.+^${}()|\\]/g, "\\$&");

/** GitHub filter glob -> RegExp (`**` any chars, `*` and `?` stop at `/`). */
function globToRegExp(glob) {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      re += ".*";
      i++;
    } else if (c === "*") re += "[^/]*";
    else if (c === "?") re += "[^/]";
    else if (c === "[" || c === "]") re += c;
    else re += escapeRegExp(c);
  }
  return new RegExp(`^${re}$`);
}

/** A pattern is banned when it spells out a banned branch and matches it. */
function namesBanned(pattern) {
  const p = pattern.startsWith("!") ? pattern.slice(1) : pattern;
  return BANNED.some((b) => {
    if (!p.includes(b)) return false;
    try {
      return globToRegExp(p).test(b);
    } catch {
      return p === b;
    }
  });
}

const keyName = (pair) => (isScalar(pair.key) ? String(pair.key.value) : undefined);

/** Return [{line, branch}] for every banned branch named in an on: filter. */
export function findBannedBranches(text) {
  const lineCounter = new LineCounter();
  const doc = parseDocument(text, { lineCounter, version: "1.2" });
  if (doc.errors.length) throw new Error(doc.errors[0].message);
  const root = doc.contents;
  if (!isMap(root)) return [];
  // `on` stays a string under YAML 1.2 (it would be `true` under YAML 1.1).
  const on = root.items.find((p) => keyName(p) === "on")?.value;
  if (!isMap(on)) return []; // `on: push` / `on: [push]` carry no filters.
  const hits = [];
  const check = (node) => {
    if (!isScalar(node) || node.value == null) return;
    const branch = String(node.value);
    if (namesBanned(branch)) hits.push({ line: lineCounter.linePos(node.range[0]).line, branch });
  };
  for (const event of on.items) {
    if (!isMap(event.value)) continue;
    for (const filter of event.value.items) {
      if (!FILTER_KEYS.has(keyName(filter))) continue;
      if (isSeq(filter.value)) filter.value.items.forEach(check);
      else check(filter.value);
    }
  }
  return hits.toSorted((a, b) => a.line - b.line);
}

/** Scan every .yml/.yaml file in `dir`; return [{file, line, branch}]. */
export function checkDir(dir) {
  return readdirSync(dir)
    .filter((f) => /\.ya?ml$/.test(f))
    .toSorted()
    .flatMap((f) => findBannedBranches(readFileSync(join(dir, f), "utf8")).map((h) => ({ file: f, ...h })));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] || join(dirname(fileURLToPath(import.meta.url)), "..", "workflows");
  const hits = checkDir(dir);
  for (const h of hits)
    console.error(`${join(dir, h.file)}:${h.line}: branch filter names "${h.branch}"; the default branch is main`);
  if (hits.length) process.exit(1);
  console.log(`OK: no workflow in ${dir} filters on ${BANNED.join(", ")}`);
}
