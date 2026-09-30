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

const changes = git("diff", "--name-status", `${base}...HEAD`).trim().split("\n").filter(Boolean)
  .map((line) => { const [status, ...rest] = line.split("\t"); return { status, file: rest[rest.length - 1] }; });

const problems = [];
for (const { status, file } of changes) {
  const rule = rules.find((r) => r.paths.some((p) => new RegExp(p).test(file)));
  if (!rule) { problems.push(`${file}: outside the allowlist${issue ? ` and ${issue}'s extras` : " (branch name has no NEWFRONT issue)"}`); continue; }
  if (rule.deletionsOnly && status !== "D") problems.push(`${file}: ${rule.source} only allows deleting files here`);
  if (rule.testidOnly) {
    const diff = git("diff", "-U0", `${base}...HEAD`, "--", file).split("\n")
      .filter((l) => /^[+-]/.test(l) && !/^(\+\+\+|---)/.test(l) && l.trim().length > 1);
    const bad = diff.filter((l) => !l.includes("data-testid"));
    if (bad.length) problems.push(`${file}: ${rule.source} only allows lines that add data-testid; found:\n    ${bad.slice(0, 5).join("\n    ")}`);
  }
}

console.log(`Issue: ${issue ?? "none"}; ${changes.length} changed file(s); rules: ${rules.map((r) => r.source).join(", ")}`);
if (problems.length) {
  console.error("Paths gate failed:\n  " + problems.join("\n  "));
  console.error("If this issue genuinely needs a path, a human adds it to .github/newfront-path-extras.json on web-new-dev (leave a process: comment on NEWFRONT-1).");
  process.exit(1);
}
console.log("Paths gate passed.");
