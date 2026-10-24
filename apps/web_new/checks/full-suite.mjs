// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs the whole web_new, @pidash/kit and @pidash/api-client unit suites on the
// current checkout and writes one JSON summary of the result. The NEWFRONT
// full-suite scheduler runs it on a fresh worktree of web-new-dev every 12
// hours (pushes to web-new-dev no longer run the web_new suite), then files
// fix issues from the summary. Failing test files are re-run once: a test that
// fails twice is "real", one that passes on the re-run is "flaky".
//
// Usage: node apps/web_new/checks/full-suite.mjs [--since <sha>] [--out <file>]
//                                                [--no-install]
//   --since <sha>  the head the previous run checked; commits after it are
//                  the merge range (default: commits of the last 12 hours)
//   --out <file>   write the summary here (default: stdout)
//   --no-install   skip pnpm install --frozen-lockfile
// Exits 0 when every suite is green, 1 when any test fails for real.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const SUITES = [
  { name: "web_new", dir: "apps/web_new" },
  { name: "kit", dir: "packages/kit" },
  { name: "api-client", dir: "packages/api-client" },
];
const EXCERPT_LINES = 30;

function parseArgs(argv) {
  const opts = { since: null, out: null, install: true };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--no-install") opts.install = false;
    else if (arg === "--since" || arg === "--out") {
      const value = argv[i + 1];
      if (!value) throw new Error(`${arg} needs a value`);
      opts[arg.slice(2)] = value;
      i += 1;
    } else throw new Error(`unknown argument ${arg}`);
  }
  return opts;
}

function excerpt(text) {
  return text.split("\n").slice(0, EXCERPT_LINES).join("\n").trim();
}

// Failures in one vitest JSON report, with repo-relative file paths. A file
// that fails to load has no assertions, only a message; it counts as one
// failing test named "(file failed to load)".
function failuresOf(report, root) {
  const failures = [];
  for (const file of report?.testResults ?? []) {
    if (file.status !== "failed") continue;
    const path = relative(root, file.name);
    const failed = (file.assertionResults ?? []).filter((test) => test.status === "failed");
    if (failed.length === 0)
      failures.push({ file: path, test: "(file failed to load)", error: excerpt(file.message ?? "") });
    for (const test of failed)
      failures.push({ file: path, test: test.fullName, error: excerpt((test.failureMessages ?? []).join("\n")) });
  }
  return failures;
}

// Test files come from testResults: vitest's numTotalTestSuites counts
// describe blocks, not files.
function countsOf(report) {
  if (!report) return { files: 0, filesFailed: 0, tests: 0, testsFailed: 0 };
  const files = report.testResults ?? [];
  return {
    files: files.length,
    filesFailed: files.filter((file) => file.status === "failed").length,
    tests: report.numTotalTests,
    testsFailed: report.numFailedTests,
  };
}

// A failure is real when the same test fails again on the re-run; a load
// failure is matched by file. Pure, so the self-test covers every branch.
function classify(first, rerun) {
  const again = new Set(rerun.map((failure) => `${failure.file}\u0000${failure.test}`));
  return first.map((failure) => ({
    ...failure,
    kind: again.has(`${failure.file}\u0000${failure.test}`) ? "real" : "flaky",
  }));
}

function uniqueFiles(failures) {
  return [...new Set(failures.map((failure) => failure.file))];
}

/* v8 ignore next 4 -- runs real processes; the self-test injects fakes */
function defaultSh(cmd, args, cwd) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 256 * 1024 * 1024,
  });
  return { status: result.status ?? 1, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

/* v8 ignore next 3 -- real git; the self-test injects a fake */
function defaultGit(args) {
  return execFileSync("git", args, { cwd: repoDir, encoding: "utf8" }).trim();
}

function readReport(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

// Runs one suite (all files, or only `files`) and returns its parsed report.
// A run that writes no report at all (vitest crashed) is a failure of the
// whole suite, kept with the tail of its output.
function runSuite(suite, files, { sh, tmp, root }) {
  const out = join(tmp, `${suite.name}-${files ? "rerun" : "all"}.json`);
  const args = [
    "exec",
    "vitest",
    "run",
    "--reporter=json",
    `--outputFile=${out}`,
    ...(files ?? []).map((file) => resolve(root, file)),
  ];
  const result = sh("pnpm", args, resolve(root, suite.dir));
  const report = readReport(out);
  if (report) return { report, crash: null };
  return { report: null, crash: excerpt(result.output.split("\n").slice(-EXCERPT_LINES).join("\n")) };
}

function mergeRange(since, git) {
  const range = since ? [`${since}..HEAD`] : ["--since=12 hours ago", "HEAD"];
  const log = git(["log", "--format=%h %s", ...range]);
  return { since, commits: log ? log.split("\n") : [] };
}

function commitsTouching(file, since, git) {
  const range = since ? [`${since}..HEAD`] : ["--since=12 hours ago", "HEAD"];
  const log = git(["log", "--format=%h %s", ...range, "--", file]);
  return log ? log.split("\n") : [];
}

function main(
  argv = process.argv.slice(2),
  {
    sh = defaultSh,
    git = defaultGit,
    root = repoDir,
    tmp = mkdtempSync(join(tmpdir(), "full-suite-")),
    write = writeFileSync,
    log = console.error,
  } = {}
) {
  const opts = parseArgs(argv);
  try {
    return runAll(opts, { sh, git, root, tmp, write, log });
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

function runAll(opts, { sh, git, root, tmp, write, log }) {
  if (opts.install) {
    const install = sh("pnpm", ["install", "--frozen-lockfile"], root);
    if (install.status !== 0)
      throw new Error(`pnpm install failed:\n${excerpt(install.output.split("\n").slice(-EXCERPT_LINES).join("\n"))}`);
  }
  const summary = { head: git(["rev-parse", "HEAD"]), green: true, suites: [], range: mergeRange(opts.since, git) };
  for (const suite of SUITES) {
    log(`full-suite: running ${suite.name}`);
    const first = runSuite(suite, null, { sh, tmp, root });
    const entry = { name: suite.name, counts: countsOf(first.report), crash: first.crash, failures: [] };
    const failed = failuresOf(first.report, root);
    if (failed.length > 0) {
      log(`full-suite: re-running ${uniqueFiles(failed).length} failing ${suite.name} file(s)`);
      const rerun = runSuite(suite, uniqueFiles(failed), { sh, tmp, root });
      entry.failures = classify(failed, failuresOf(rerun.report, root)).map((failure) => ({
        ...failure,
        commits: commitsTouching(failure.file, opts.since, git),
      }));
    }
    if (entry.crash || entry.failures.some((failure) => failure.kind === "real")) summary.green = false;
    summary.suites.push(entry);
  }
  const json = `${JSON.stringify(summary, null, 2)}\n`;
  if (opts.out) write(opts.out, json);
  else process.stdout.write(json);
  log(`full-suite: ${summary.head.slice(0, 9)} ${summary.green ? "green" : "red"}`);
  return summary.green ? 0 : 1;
}

// True when this module is the process entry point. Exported so the self-test
// can cover it without relying on a child process's coverage.
function isCliEntry(argv1 = process.argv[1]) {
  return argv1 !== undefined && import.meta.url === pathToFileURL(argv1).href;
}

/* v8 ignore next 3 -- child-process bootstrap; the scheduler runs it */
if (isCliEntry()) {
  process.exit(main());
}

export { classify, countsOf, excerpt, failuresOf, isCliEntry, main, parseArgs, uniqueFiles };
