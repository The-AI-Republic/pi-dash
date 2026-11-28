// Workflow branch-filter guard. The default branch is `main`; a workflow whose
// `on.<event>.branches` / `branches-ignore` filter names `master` never fires
// (or compares against a branch that does not exist). The workflow is parsed
// as YAML, so only real trigger filters count: quoted keys and multi-line flow
// lists are caught, YAML aliases are followed, and text inside `run:` scripts
// or `with:` inputs is not. A filter is flagged when it matches `master` but
// not `main` under GitHub's filter-pattern rules.
// Needs the pinned `yaml` package: npm ci --prefix .github/scripts
// Exits 1 and lists every offending file:line.
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { LineCounter, isAlias, isMap, isScalar, isSeq, parseDocument } from "yaml";

const BANNED = ["master"];
const FILTER_KEYS = new Set(["branches", "branches-ignore"]);

const escapeRegExp = (s) => s.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");

/**
 * GitHub filter pattern -> RegExp, per the Actions filter-pattern cheat sheet:
 * `**` any chars, `*` any chars but `/`, `?` / `+` make the preceding
 * character optional / repeatable, `[...]` one character, `\` escapes.
 */
export function globToRegExp(glob) {
  const tokens = [];
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      tokens.push(".*");
      i++;
    } else if (c === "*") tokens.push("[^/]*");
    else if ((c === "?" || c === "+") && tokens.length) tokens.push(`(?:${tokens.pop()})${c}`);
    else if (c === "[" && glob.indexOf("]", i + 1) > i + 1) {
      const end = glob.indexOf("]", i + 1);
      tokens.push(`[${glob.slice(i + 1, end).replace(/[\\\]^]/g, "\\$&")}]`);
      i = end;
    } else if (c === "\\" && i + 1 < glob.length) tokens.push(escapeRegExp(glob[++i]));
    else tokens.push(escapeRegExp(c));
  }
  return new RegExp(`^${tokens.join("")}$`);
}

/**
 * A pattern is banned when it targets a banned branch instead of main: it
 * matches `master` but not `main`. Broad globs such as `*` or `ma*` still
 * reach main, so they are fine.
 */
function namesBanned(pattern) {
  const p = pattern.startsWith("!") ? pattern.slice(1) : pattern;
  let re;
  try {
    re = globToRegExp(p);
  } catch {
    return BANNED.includes(p);
  }
  return !re.test("main") && BANNED.some((b) => re.test(b));
}

const keyName = (pair) => (isScalar(pair.key) ? String(pair.key.value) : undefined);

/** Return [{line, branch}] for every banned branch named in an on: filter. */
export function findBannedBranches(text) {
  const lineCounter = new LineCounter();
  const doc = parseDocument(text, { lineCounter, version: "1.2" });
  if (doc.errors.length) throw new Error(doc.errors[0].message);
  const lineOf = (node) => lineCounter.linePos(node.range[0]).line;
  // Follow `*alias` to its anchored node. A hit found through an alias is
  // reported on the alias's line, where the workflow uses it.
  const deref = (node, at) => (isAlias(node) ? [node.resolve(doc), at ?? lineOf(node)] : [node, at]);
  const [root] = deref(doc.contents);
  if (!isMap(root)) return [];
  // `on` stays a string under YAML 1.2 (it would be `true` under YAML 1.1).
  const [on, onAt] = deref(root.items.find((p) => keyName(p) === "on")?.value);
  if (!isMap(on)) return []; // `on: push` / `on: [push]` carry no filters.
  const hits = [];
  const check = (raw, at) => {
    const [node, line] = deref(raw, at);
    if (isSeq(node)) return node.items.forEach((item) => check(item, line));
    if (!isScalar(node) || node.value == null) return;
    const branch = String(node.value);
    if (namesBanned(branch)) hits.push({ line: line ?? lineOf(node), branch });
  };
  for (const pair of on.items) {
    const [event, eventAt] = deref(pair.value, onAt);
    if (!isMap(event)) continue;
    for (const filter of event.items) {
      if (FILTER_KEYS.has(keyName(filter))) check(filter.value, eventAt);
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
