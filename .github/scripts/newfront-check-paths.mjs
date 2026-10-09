// NEWFRONT paths gate. Human-maintained; loaded from the base branch by
// .github/workflows/web-new-paths.yml so a PR cannot change its own rules.
// Every file a PR touches must match the base allowlist, or an extra granted
// to the PR's issue in .github/newfront-path-extras.json (also read from base).
import { execFileSync } from "node:child_process";

const git = (...args) => execFileSync("git", args, { encoding: "utf8" });
const base = `origin/${process.env.BASE_REF || "web-new-dev"}`;
const headRef = process.env.HEAD_REF || "";

const cfg = JSON.parse(git("show", `${base}:.github/newfront-path-extras.json`));
const m = headRef.match(/newfront-(\d+)/i);
const issue = m ? `NEWFRONT-${m[1]}` : null;

const rules = [{ source: "NEWFRONT-1 allowlist", paths: cfg.base }];
if (issue && cfg.issues[issue]) rules.push({ source: `${issue} (${cfg.issues[issue].reason})`, ...cfg.issues[issue] });
if (issue && cfg.oracleIssues.ids.includes(issue)) rules.push({ source: `${issue} (oracle selectors)`, ...cfg.oracleIssues });

// A file with every data-testid attribute removed and all whitespace dropped. Two versions
// of a file that differ only by data-testid attributes (and the line breaks a formatter adds
// around them) reduce to the same string.
const withoutTestids = (src) => {
  let out = "";
  let i = 0;
  const attr = /\s*\bdata-testid=/g;
  for (;;) {
    attr.lastIndex = i;
    const m = attr.exec(src);
    if (!m) break;
    let j = attr.lastIndex;
    const open = src[j];
    if (open === '"' || open === "'") {
      j = src.indexOf(open, j + 1) + 1;
      if (j === 0) break;
    } else if (open === "{") {
      let depth = 0;
      do {
        if (src[j] === "{") depth++;
        else if (src[j] === "}") depth--;
        j++;
      } while (depth > 0 && j < src.length);
    } else {
      out += src.slice(i, j);
      i = j;
      continue;
    }
    out += src.slice(i, m.index);
    i = j;
  }
  return (out + src.slice(i)).replace(/\s+/g, "");
};
const show = (rev, file) => {
  try {
    return git("show", `${rev}:${file}`);
  } catch {
    return null;
  }
};
const mergeBase = git("merge-base", base, "HEAD").trim();

const changes = git("diff", "--name-status", `${base}...HEAD`).trim().split("\n").filter(Boolean)
  .map((line) => { const [status, ...rest] = line.split("\t"); return { status, file: rest[rest.length - 1] }; });

const problems = [];
for (const { status, file } of changes) {
  const rule = rules.find((r) => r.paths.some((p) => new RegExp(p).test(file)));
  if (!rule) { problems.push(`${file}: outside the allowlist${issue ? ` and ${issue}'s extras` : " (branch name has no NEWFRONT issue)"}`); continue; }
  if (rule.deletionsOnly && status !== "D") problems.push(`${file}: ${rule.source} only allows deleting files here`);
  if (rule.testidOnly) {
    const before = show(mergeBase, file);
    const after = show("HEAD", file);
    if (before === null || after === null) {
      problems.push(`${file}: ${rule.source} only allows adding data-testid to existing files, not adding or deleting files`);
    } else {
      const a = withoutTestids(before);
      const b = withoutTestids(after);
      if (a !== b) {
        let k = 0;
        while (k < a.length && a[k] === b[k]) k++;
        problems.push(
          `${file}: ${rule.source} only allows adding data-testid attributes; something else changed near:\n    was: ${a.slice(Math.max(0, k - 40), k + 60)}\n    now: ${b.slice(Math.max(0, k - 40), k + 60)}`,
        );
      }
    }
  }
}

console.log(`Issue: ${issue ?? "none"}; ${changes.length} changed file(s); rules: ${rules.map((r) => r.source).join(", ")}`);
if (problems.length) {
  console.error("Paths gate failed:\n  " + problems.join("\n  "));
  console.error("If this issue genuinely needs a path, a human adds it to .github/newfront-path-extras.json on web-new-dev (leave a process: comment on NEWFRONT-1).");
  process.exit(1);
}
console.log("Paths gate passed.");
