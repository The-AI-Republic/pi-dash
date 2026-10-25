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
//                  the merge range (default, or when the SHA is unknown:
//                  commits of the last 12 hours)
//   --out <file>   write the summary here (default: stdout)
//   --no-install   skip pnpm install --frozen-lockfile
// Exits 0 when every suite is green, 1 when a test fails for real or a suite
// crashes, and 2 when the script itself fails (bad arguments, a failed
// install); the summary then holds only { error }.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir, totalmem } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const SUITES = [
  { name: "web_new", dir: "apps/web_new" },
  { name: "kit", dir: "packages/kit" },
  { name: "api-client", dir: "packages/api-client" },
];
const EXCERPT_LINES = 30;
// A suite still running after this is stopped and reported as a crash.
const SUITE_TIMEOUT_MS = 45 * 60 * 1000;
const LOAD_FAILURE = "(file failed to load)";
// A forked worker needs its own V8 heap and module graph. Reserving this much
// memory per worker keeps a larger pool from forcing the host into swap, where
// files that normally pass start tripping their individual 30 s test timeouts.
const WORKER_MEMORY_GIB = 1.5;
const WORKER_CAP = 16;

// Vitest uses one forked worker per CPU by default. A larger pool helps the
// shared 4-vCPU scheduler host, where the suite spends much of its wall time
// waiting on contended cores, but the same pool doubles the footprint on an
// 8-core / 8 GiB host and turns green tests into `Test timed out`. Oversubscribe
// only as far as the host's memory can support, and otherwise leave vitest's
// default untouched.
function workerArgs({ cpus, memoryGiB }) {
  const memoryWorkers = Math.floor(memoryGiB / WORKER_MEMORY_GIB);
  const workers = Math.min(WORKER_CAP, Math.max(cpus, memoryWorkers));
  return workers > cpus ? [`--maxWorkers=${workers}`] : [];
}

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

function tail(output) {
  return excerpt(output.split("\n").slice(-EXCERPT_LINES).join("\n"));
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
    if (failed.length === 0) failures.push({ file: path, test: LOAD_FAILURE, error: excerpt(file.message ?? "") });
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

// A failure is real when it fails again on the re-run: the same test, or, for
// a file that failed to load in either run, any failure in that file. When the
// re-run itself crashed nothing was disproved, so every failure stays real.
// Pure, so the self-test covers every branch.
function classify(first, rerun, rerunCrashed = false) {
  const again = new Set(rerun.map((failure) => `${failure.file}\u0000${failure.test}`));
  const failingFiles = new Set(rerun.map((failure) => failure.file));
  const unloadedFiles = new Set(
    rerun.filter((failure) => failure.test === LOAD_FAILURE).map((failure) => failure.file)
  );
  return first.map((failure) => {
    const real =
      rerunCrashed ||
      again.has(`${failure.file}\u0000${failure.test}`) ||
      unloadedFiles.has(failure.file) ||
      (failure.test === LOAD_FAILURE && failingFiles.has(failure.file));
    return { ...failure, kind: real ? "real" : "flaky" };
  });
}

function uniqueFiles(failures) {
  return [...new Set(failures.map((failure) => failure.file))];
}

/* v8 ignore next 14 -- runs real processes; the self-test injects fakes */
function defaultSh(cmd, args, cwd, { timeoutMs } = {}) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 256 * 1024 * 1024,
    timeout: timeoutMs,
  });
  return {
    status: result.status ?? 1,
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`,
    timedOut: result.error?.code === "ETIMEDOUT",
  };
}

/* v8 ignore next 3 -- real git; the self-test injects a fake */
function defaultGit(args) {
  return execFileSync("git", args, { cwd: repoDir, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function readReport(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

// Runs one suite (all files, or only `files`) and returns its parsed report
// and a crash, if any. A crash is a run that timed out, wrote no report, or
// failed (non-zero exit, or success: false for unhandled errors or no test
// files) without a failing test to show for it. It keeps the output's tail.
function runSuite(suite, files, { sh, tmp, root, host }) {
  const out = join(tmp, `${suite.name}-${files ? "rerun" : "all"}.json`);
  // The default reporter is what makes a timeout diagnosable: with only the
  // JSON reporter every line of test output goes to the report file, which a
  // killed run never writes, so the crash carried no evidence at all. The
  // default reporter's progress tail names the file that was still in flight.
  const args = [
    "exec",
    "vitest",
    "run",
    "--reporter=default",
    "--reporter=json",
    `--outputFile=${out}`,
    ...(suite.name === "web_new" ? workerArgs(host) : []),
    ...(files ?? []).map((file) => resolve(root, file)),
  ];
  const result = sh("pnpm", args, resolve(root, suite.dir), { timeoutMs: SUITE_TIMEOUT_MS });
  const report = readReport(out);
  if (result.timedOut)
    return { report, crash: `timed out after ${SUITE_TIMEOUT_MS / 60000} min\n${tail(result.output)}` };
  if (!report) return { report: null, crash: tail(result.output) };
  const failed = result.status !== 0 || report.success === false;
  if (failed && failuresOf(report, root).length === 0) return { report, crash: tail(result.output) };
  return { report, crash: null };
}

// One-line commits after `since` (or of the last 12 hours without one),
// optionally only those touching `paths`.
function commitLog(git, since, ...paths) {
  const range = since ? [`${since}..HEAD`] : ["--since=12 hours ago", "HEAD"];
  const log = git(["log", "--format=%h %s", ...range, ...(paths.length > 0 ? ["--", ...paths] : [])]);
  return log ? log.split("\n") : [];
}

// A --since SHA that no longer exists (e.g. after a force-push) falls back to
// the last 12 hours instead of failing the run.
function knownSince(since, git, log) {
  if (!since) return null;
  try {
    git(["rev-parse", "--verify", "--quiet", `${since}^{commit}`]);
    return since;
  } catch {
    log(`full-suite: --since ${since} is not a known commit; using the last 12 hours`);
    return null;
  }
}

function emit(summary, opts, write) {
  const json = `${JSON.stringify(summary, null, 2)}\n`;
  if (opts.out) write(opts.out, json);
  else process.stdout.write(json);
}

function runAll(opts, { sh, git, root, tmp, write, log, host }) {
  if (opts.install) {
    const install = sh("pnpm", ["install", "--frozen-lockfile"], root, { timeoutMs: SUITE_TIMEOUT_MS });
    if (install.status !== 0 || install.timedOut) throw new Error(`pnpm install failed:\n${tail(install.output)}`);
  }
  const since = knownSince(opts.since, git, log);
  const summary = {
    head: git(["rev-parse", "HEAD"]),
    green: true,
    suites: [],
    range: { since, requestedSince: opts.since, commits: commitLog(git, since) },
  };
  for (const suite of SUITES) {
    log(`full-suite: running ${suite.name}`);
    const first = runSuite(suite, null, { sh, tmp, root, host });
    const entry = {
      name: suite.name,
      counts: countsOf(first.report),
      crash: first.crash,
      rerunCrash: null,
      failures: [],
    };
    const failed = failuresOf(first.report, root);
    if (failed.length > 0) {
      const files = uniqueFiles(failed);
      log(`full-suite: re-running ${files.length} failing ${suite.name} file(s)`);
      const rerun = runSuite(suite, files, { sh, tmp, root, host });
      entry.rerunCrash = rerun.crash;
      const commits = new Map(files.map((file) => [file, commitLog(git, since, file)]));
      entry.failures = classify(failed, failuresOf(rerun.report, root), rerun.crash !== null).map((failure) => ({
        ...failure,
        commits: commits.get(failure.file),
      }));
    }
    if (entry.crash || entry.failures.some((failure) => failure.kind === "real")) summary.green = false;
    summary.suites.push(entry);
  }
  emit(summary, opts, write);
  log(`full-suite: ${summary.head.slice(0, 9)} ${summary.green ? "green" : "red"}`);
  return summary.green ? 0 : 1;
}

function main(
  argv = process.argv.slice(2),
  {
    sh = defaultSh,
    git = defaultGit,
    root = repoDir,
    tmp = null,
    write = writeFileSync,
    log = console.error,
    host = { cpus: availableParallelism(), memoryGiB: totalmem() / 1024 ** 3 },
  } = {}
) {
  let opts = { out: null };
  let status;
  const dir = tmp ?? mkdtempSync(join(tmpdir(), "full-suite-"));
  try {
    opts = parseArgs(argv);
    status = runAll(opts, { sh, git, root, tmp: dir, write, log, host });
  } catch (error) {
    log(`full-suite: error: ${error.message}`);
    emit({ error: error.message }, opts, write);
    status = 2;
  }
  rmSync(dir, { recursive: true, force: true });
  return status;
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

export { classify, countsOf, excerpt, failuresOf, isCliEntry, main, parseArgs, uniqueFiles, workerArgs };
