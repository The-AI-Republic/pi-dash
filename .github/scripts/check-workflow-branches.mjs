// Workflow branch-filter guard (PIDASH-1). The default branch is `main`; a
// workflow whose `branches:` / `branches-ignore:` filter names `master` never
// fires (or compares against a branch that does not exist). Dependency-free so
// CI can run it with a bare `node`. Exits 1 and lists every offending line.
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const BANNED = new Set(["master"]);
const KEY = /^(\s*)(?:-\s+)?branches(?:-ignore)?\s*:\s*(.*)$/;

const stripComment = (s) => s.replace(/\s+#.*$/, "").replace(/^#.*$/, "").trim();
const unquote = (s) => s.trim().replace(/^(['"])(.*)\1$/, "$2");

/** Return [{line, branch}] for every banned branch named in a branch filter. */
export function findBannedBranches(text) {
  const lines = text.split(/\r?\n/);
  const hits = [];
  const check = (value, lineNo) => {
    const branch = unquote(value);
    if (BANNED.has(branch)) hits.push({ line: lineNo, branch });
  };
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(KEY);
    if (!m) continue;
    const indent = m[1].length;
    const rest = stripComment(m[2]);
    if (rest.startsWith("[")) {
      rest.replace(/^\[|\]$/g, "").split(",").forEach((v) => check(v, i + 1));
    } else if (rest) {
      check(rest, i + 1);
    } else {
      // Block list: following `- item` lines indented at least as far as the key.
      for (let j = i + 1; j < lines.length; j++) {
        const raw = lines[j];
        if (!raw.trim() || raw.trim().startsWith("#")) continue;
        const item = raw.match(/^(\s*)-\s+(.*)$/);
        if (!item || item[1].length < indent) break;
        check(stripComment(item[2]), j + 1);
      }
    }
  }
  return hits;
}

/** Scan every .yml/.yaml file in `dir`; return [{file, line, branch}]. */
export function checkDir(dir) {
  return readdirSync(dir)
    .filter((f) => /\.ya?ml$/.test(f))
    .sort()
    .flatMap((f) => findBannedBranches(readFileSync(join(dir, f), "utf8")).map((h) => ({ file: f, ...h })));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] || join(dirname(fileURLToPath(import.meta.url)), "..", "workflows");
  const hits = checkDir(dir);
  for (const h of hits) console.error(`${join(dir, h.file)}:${h.line}: branch filter names "${h.branch}"; the default branch is main`);
  if (hits.length) process.exit(1);
  console.log(`OK: no workflow in ${dir} filters on ${[...BANNED].join(", ")}`);
}
