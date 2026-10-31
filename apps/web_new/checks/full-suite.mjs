// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs the whole web_new, @pidash/kit and @pidash/api-client unit suites on the
// current checkout and writes one JSON summary of the result. The NEWFRONT
// full-suite scheduler runs it on a fresh worktree of main every 12
// hours (pushes to main do not run the web_new suite), then files
// fix issues from the summary. Failing test files are re-run once: a test that
// fails twice is "real", one that passes on the re-run is "flaky".
//
// web_new has outgrown a single process: one 19 000-test run spends most of
// its wall time importing modules, and a fork pool that keeps one worker per
// file grows its heaps until a memory-tight host swaps (measured: 0.37 s per
// file in a short run, 2.7 s in a long one on the same host). So the suite runs
// in bounded batches. Each batch appends a line per finished file to a progress
// file, so a batch stopped at its deadline still says which files passed and
// which were in flight; stock vitest only writes its JSON at the end, which is
// how the timeout at 226b49807 produced `files=0` with no file names. A file
// that never finishes is bisected out of its batch and reported as a real
// failure, so the next red run names it.
//
// Usage: node apps/web_new/checks/full-suite.mjs [--since <sha>] [--out <file>]
//                                                [--no-install]
//   --since <sha>  the head the previous run checked; commits after it are
//                  the merge range (default, or when the SHA is unknown:
//                  commits of the last 12 hours)
//   --out <file>   write the summary here (default: stdout)
//   --no-install   skip pnpm install --frozen-lockfile
//
// web_new runs in batches, each with its own deadline and its own incremental
// per-file progress file, so a suite that misses its deadline still reports
// which files passed and which never finished.
// Exits 0 when every suite is green, 1 when a test fails for real or a suite
// crashes, and 2 when the script itself fails (bad arguments, a failed
// install); the summary then holds only { error }.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, loadavg as osLoadavg, tmpdir, totalmem } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const EXCERPT_LINES = 30;
// A suite still running after this is stopped and reported as a crash.
const SUITE_TIMEOUT_MS = 45 * 60 * 1000;
// The scheduler stops the whole checker at 45 minutes. Every suite shares one
// budget that ends short of that, so the summary is written before the stop: a
// suite that spends the budget is reported with the files it never reached,
// instead of the process being killed with no summary at all (`files=0`).
const TOTAL_BUDGET_MS = 42 * 60 * 1000;
// `pnpm install` is not test work and cannot be allowed to eat the budget.
const INSTALL_TIMEOUT_MS = 10 * 60 * 1000;
const LOAD_FAILURE = "(file failed to load)";
// The reporter that streams one line per finished test file (see
// full-suite-progress.mjs), and the environment variable it reads its output
// path from.
const PROGRESS_REPORTER = join(dirname(fileURLToPath(import.meta.url)), "full-suite-progress.mjs");
const PROGRESS_ENV = "FULL_SUITE_PROGRESS";
// web_new has outlined one process's worth of work. Batches bound how long a
// stuck file can go unnoticed and keep each worker's heap small; the sizes are
// files per batch, and one batch's first allowance follows from the suite
// budget so a single batch can still use most of it.
const BATCH_FILES = 200;
const BATCH_FILES_WEB_NEW = 120;
// Kept for the file listing call, which must not hang either.
const BATCH_TIMEOUT_MS = 15 * 60 * 1000;
const BATCH_SLOW_MS = 4 * 60 * 1000;
const BATCH_FAST_MS = 60 * 1000;
const BATCH_MIN_FILES = 20;
const BATCH_MAX_FILES = 400;
const FIRST_BATCH_TIMEOUT_MS = 12 * 60 * 1000;
const BATCH_TIMEOUT_FACTOR = 3;
const MIN_BATCH_TIMEOUT_MS = 3 * 60 * 1000;
const MAX_BATCH_TIMEOUT_MS = 20 * 60 * 1000;
// A file that outlives its own batch timeout after this many halvings is the
// stuck group; naming it is more useful than bisecting forever.
const BISECT_ROUNDS = 3;
const HUNG_FAILURE = "(file did not finish)";
const SUITES = [
  { name: "web_new", dir: "apps/web_new", batchFiles: BATCH_FILES_WEB_NEW },
  { name: "kit", dir: "packages/kit", batchFiles: BATCH_FILES },
  { name: "api-client", dir: "packages/api-client", batchFiles: BATCH_FILES },
];
// A forked worker needs its own V8 heap and module graph. Reserving this much
// memory per worker keeps a larger pool from forcing the host into swap, where
// files that normally pass start tripping their individual 30 s test timeouts.
const WORKER_MEMORY_GIB = 1.5;
const WORKER_CAP = 16;

// How many fork workers web_new gets. The suite is CPU-bound (transform plus
// jsdom), so the pool serves two budgets that pull in opposite directions.
// The per-test budget wants short runqueues: a 120-file batch on 4 pinned
// CPUs takes the same wall time at 16 workers as at 4 (60 s vs 58 s), while
// every test's wall time grows with the runqueue (measured p50/p99/max
// 30 ms/1.4 s/6.0 s at 16 workers vs 7 ms/0.4 s/1.6 s at 4, slowest file 35 s
// vs 9 s). On a quiet host the extra workers are pure tail latency, and a
// transient spike then trips unrelated suites (NEWFRONT-985: 7 timeouts, 3 in
// fully synchronous tests, all green on re-run). The 45-minute budget wants
// CPU share under sustained contention instead: on a busy shared host 16
// workers hold ~2 of 4 cores and finish in 38 minutes, while 4 would take
// twice the budget (NEWFRONT-893). So a quiet host runs one worker per CPU
// and a contended host oversubscribes up to the cap; memory only ever shrinks
// the pool, never grows it. Batches (see runSuiteFiles) are what keep one
// run's heaps from growing across the whole suite; the pool only sets the
// width. Pure, so the self-test covers every branch.
// `externalLoad` is the host's sustained load outside this run (the 5-minute
// average minus the workers the previous batch ran): it follows a regime that
// lasts minutes and ignores a transient spike, which is exactly when the
// small pool must hold.
function workerCount({ cpus, memoryGiB, externalLoad }) {
  const memoryWorkers = Math.max(1, Math.floor(memoryGiB / WORKER_MEMORY_GIB));
  const cap = Math.min(WORKER_CAP, memoryWorkers);
  if (externalLoad < cpus) return Math.min(cpus, cap);
  return cap;
}

// The vitest flag for the pool, or nothing when it matches vitest's default
// of one worker per CPU.
function workerArgs(host) {
  const workers = workerCount(host);
  return workers === host.cpus ? [] : [`--maxWorkers=${workers}`];
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
// a file that failed to load in either run, any failure in that file. A file
// that did not finish either run is also real: a hang is not flakiness, and a
// first-run hang whose re-run fails a real test must not turn the suite green.
// When the re-run itself crashed nothing was disproved, so every failure stays
// real.
// Pure, so the self-test covers every branch.
function classify(first, rerun, rerunCrashed = false) {
  const again = new Set(rerun.map((failure) => `${failure.file}\u0000${failure.test}`));
  const failingFiles = new Set(rerun.map((failure) => failure.file));
  const unloadedFiles = new Set(
    rerun.filter((failure) => failure.test === LOAD_FAILURE).map((failure) => failure.file)
  );
  const hungFiles = new Set(rerun.filter((failure) => failure.test === HUNG_FAILURE).map((failure) => failure.file));
  return first.map((failure) => {
    const real =
      rerunCrashed ||
      again.has(`${failure.file}\u0000${failure.test}`) ||
      unloadedFiles.has(failure.file) ||
      hungFiles.has(failure.file) ||
      (failure.test === HUNG_FAILURE && failingFiles.has(failure.file)) ||
      (failure.test === LOAD_FAILURE && failingFiles.has(failure.file));
    return { ...failure, kind: real ? "real" : "flaky" };
  });
}

function uniqueFiles(failures) {
  return [...new Set(failures.map((failure) => failure.file))];
}

/* v8 ignore next 14 -- runs real processes; the self-test injects fakes */
function defaultSh(cmd, args, cwd, { timeoutMs, env } = {}) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 256 * 1024 * 1024,
    timeout: timeoutMs,
    env: env ? { ...process.env, ...env } : process.env,
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

function readText(file) {
  try {
    return readFileSync(file, "utf8");
  } catch {
    return "";
  }
}

// `vitest list --filesOnly` prints one file path per line; blank lines and
// progress markers are not files.
function filesOf(output) {
  return output
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !/\s/.test(line) && !line.startsWith("·"));
}

function chunksOf(list, size) {
  const chunks = [];
  for (let i = 0; i < list.length; i += size) chunks.push(list.slice(i, i + size));
  return chunks;
}

// The progress reporter's JSON lines. A half-written last line (the run was
// killed mid-write) is dropped rather than failing the parse.
function progressOf(text) {
  const finished = new Map();
  for (const raw of text.split("\n")) {
    if (!raw.trim()) continue;
    let event;
    try {
      event = JSON.parse(raw);
    } catch {
      continue;
    }
    if (event?.event === "end" && typeof event.file === "string") finished.set(event.file, event);
  }
  return finished;
}

// A vitest-shaped result for a file the JSON report does not cover but the
// progress file saw finish. Failed tests keep the names and messages the
// reporter recorded, so a classification made from this result names the same
// test the JSON report would. Older lines carry only counts; a failure that is
// all the file has becomes the file-level failure marker.
function progressResult(absolute, event) {
  const failed = event.state === "failed";
  const failures = Array.isArray(event.failures) ? event.failures : [];
  const passed = event.passed ?? 0;
  const skipped = event.skipped ?? 0;
  const assertions = Array.from({ length: passed }, (_, i) => ({
    fullName: `${absolute} passed #${i + 1}`,
    status: "passed",
    failureMessages: [],
  }));
  for (const failure of failures) {
    assertions.push({
      fullName: failure.name || `${absolute} failed`,
      status: "failed",
      failureMessages: [excerpt(failure.message ?? "")],
    });
  }
  if (failed && assertions.every((assertion) => assertion.status !== "failed")) {
    assertions.push({ fullName: HUNG_FAILURE, status: "failed", failureMessages: [excerpt(event.message ?? "")] });
  }
  for (let i = 0; i < skipped; i += 1) {
    assertions.push({ fullName: `${absolute} skipped #${i + 1}`, status: "pending", failureMessages: [] });
  }
  return {
    name: absolute,
    status: failed ? "failed" : "passed",
    message: event.message ?? "",
    startTime: 0,
    endTime: 0,
    assertionResults: assertions,
  };
}

function failureResult(absolute, message) {
  return {
    name: absolute,
    status: "failed",
    message,
    startTime: 0,
    endTime: 0,
    assertionResults: [{ fullName: HUNG_FAILURE, status: "failed", failureMessages: [message] }],
  };
}

// Per-file results for one batch: the JSON report when the batch finished, the
// progress line when only that file finished, otherwise it is `missing`.
function resultsOfBatch(batch, absoluteOf, report, finished) {
  const byName = new Map((report?.testResults ?? []).map((file) => [file.name, file]));
  const results = [];
  const missing = [];
  for (const file of batch) {
    const absolute = absoluteOf(file);
    const reported = byName.get(absolute);
    if (reported) results.push(reported);
    else if (finished.has(absolute)) results.push(progressResult(absolute, finished.get(absolute)));
    else missing.push(file);
  }
  return { results, missing };
}

// A report assembled from per-file results, so a suite that ran in several
// batches (or was stopped part-way) still yields counts and failures.
function reportFromFiles(results) {
  const tests = results.flatMap((file) => file.assertionResults ?? []);
  const failed = tests.filter((test) => test.status === "failed").length;
  const filesFailed = results.filter((file) => file.status === "failed").length;
  return {
    success: results.length > 0 && failed === 0 && filesFailed === 0,
    numTotalTests: tests.length,
    numFailedTests: failed,
    testResults: results,
  };
}

// Runs one batch and returns its JSON report (null when it did not finish),
// the progress lines it wrote, the pool it ran with, and how it ended.
function runBatch(
  suite,
  files,
  { sh, tmp, root, host, loadavg = osLoadavg, lastWorkers = 0, timeoutMs, seq, label = "all" }
) {
  const out = join(tmp, `${suite.name}-${label}-${seq}.json`);
  const progress = join(tmp, `${suite.name}-${label}-${seq}.jsonl`);
  rmSync(progress, { force: true });
  // Sustained load outside this run: our own previous batch is still inside
  // the 5-minute average, so it is subtracted back out (clamped: a batch
  // shorter than a minute never contributed its full width).
  const externalLoad = suite.name === "web_new" ? Math.max(0, loadavg()[1] - lastWorkers) : 0;
  const pool = suite.name === "web_new" ? workerArgs({ ...host, externalLoad }) : [];
  const args = [
    "exec",
    "vitest",
    "run",
    "--reporter=default",
    "--reporter=json",
    `--outputFile=${out}`,
    `--reporter=${PROGRESS_REPORTER}`,
    ...pool,
    // `vitest list` prints paths relative to the suite's directory.
    ...files.map((file) => resolve(root, suite.dir, file)),
  ];
  const startedAt = Date.now();
  const result = sh("pnpm", args, resolve(root, suite.dir), {
    timeoutMs,
    env: { [PROGRESS_ENV]: progress },
  });
  return {
    report: readReport(out),
    finished: progressOf(readText(progress)),
    timedOut: result.timedOut,
    status: result.status,
    output: result.output,
    timeoutMs,
    elapsedMs: Date.now() - startedAt,
    workers: suite.name === "web_new" ? workerCount({ ...host, externalLoad }) : host.cpus,
  };
}

// A run that exited non-zero without naming a failing test is a crash: an
// unhandled rejection, a worker that died, or no test files at all. Its output
// tail is the only evidence, so it must reach the summary.
function crashedRun(run, root) {
  if (run.status === 0 && run.report.success !== false) return false;
  return failuresOf(run.report, root).length === 0;
}

// Every test file the suite's config selects. The listing is bounded by the
// suite's remaining budget as well as its own timeout.
function listFiles(suite, { sh, root, timeoutMs = BATCH_TIMEOUT_MS }) {
  const result = sh("pnpm", ["exec", "vitest", "list", "--filesOnly"], resolve(root, suite.dir), { timeoutMs });
  return result.status === 0 ? filesOf(result.output) : null;
}

// Batch sizing adapts to the host: a slow batch shrinks (so less budget is
// lost to a stuck file and the progress file is refreshed more often), a fast
// one grows so worker startup is paid fewer times. Pure.
function nextBatchSize(size, elapsedMs) {
  if (elapsedMs > BATCH_SLOW_MS) return Math.max(BATCH_MIN_FILES, Math.floor(size / 2));
  if (elapsedMs < BATCH_FAST_MS) return Math.min(BATCH_MAX_FILES, size * 2);
  return size;
}

// A batch far slower than the last one, or than the first-batch allowance, is
// stopped: a file that never settles must not spend the whole suite budget
// before anyone learns its name. Pure.
function nextBatchTimeout(lastElapsedMs) {
  if (lastElapsedMs == null) return FIRST_BATCH_TIMEOUT_MS;
  return Math.min(MAX_BATCH_TIMEOUT_MS, Math.max(MIN_BATCH_TIMEOUT_MS, lastElapsedMs * BATCH_TIMEOUT_FACTOR));
}

// When one suite must stop: its own allowance, or the run's shared budget,
// whichever ends first. Pure.
function suiteDeadline(budgetEndsAt, now = Date.now()) {
  return Math.min(budgetEndsAt, now + SUITE_TIMEOUT_MS);
}

// A re-run's allowance, clipped to the run's shared budget so a failure found
// in the last batch cannot push the summary past it. Pure.
function rerunDeadline(fileCount, budgetEndsAt, now = Date.now()) {
  return Math.min(budgetEndsAt, now + nextRerunTimeout(fileCount));
}

// A repo-relative path (`apps/web_new/src/a.test.ts`) becomes the suite-
// relative one `vitest list` prints (`src/a.test.ts`).
function suiteRelative(suite, file) {
  const prefix = `${suite.dir}/`;
  return file.startsWith(prefix) ? file.slice(prefix.length) : file;
}

// Runs `batch`, and when it stops without naming every file, narrows down what
// did not finish: one file alone is retried once, a larger group is split in
// halves until the halving limit. Whatever still does not finish comes back as
// a failure named "(file did not finish)", so the next red run names a file
// instead of reporting a bare timeout.
function runBatchIsolated(suite, batch, ctx, depth = 0) {
  const remaining = ctx.deadline - Date.now();
  if (remaining <= 0) return { results: [], unfinished: [...batch], runs: 0, crashed: false, output: "" };
  const timeoutMs = Math.min(nextBatchTimeout(ctx.lastElapsedMs), remaining);
  const run = runBatch(suite, batch, { ...ctx, timeoutMs });
  const absoluteOf = (file) => resolve(ctx.root, suite.dir, file);
  const { results, missing } = resultsOfBatch(batch, absoluteOf, run.report, run.finished);
  ctx.lastElapsedMs = run.elapsedMs;
  ctx.lastWorkers = run.workers;
  // A run that exited non-zero with no failing test to show is a crash: an
  // unhandled rejection, a worker that died, or no test files. It has to reach
  // the summary even though every file it did report passed.
  if (run.report || missing.length === 0) {
    // A non-zero run with no JSON report is a crash even when every file
    // streamed a passing progress line: the process did not finish cleanly, so
    // treating it as green would hide a worker that died after the last module.
    const crashed = run.timedOut || (!run.report && run.status !== 0) || crashedRun(run, ctx.root);
    return { results, unfinished: [], runs: 1, crashed, output: run.output };
  }
  // The batch stopped without timing out, so it died: an early crash is not a
  // hang, and retrying the unfinished files would only repeat it. The output
  // tail is the evidence; the files it never reached stay unaccounted for.
  if (!run.timedOut) return { results, unfinished: [], runs: 1, crashed: true, output: run.output };
  if (missing.length === 1) {
    // One file, alone, still did not finish: that is the stuck file.
    const message =
      `${HUNG_FAILURE}\n${missing[0]} outlived a ${Math.round(timeoutMs / 1000)} s batch on its own\n${tail(run.output)}`.trim();
    return {
      results: [...results, failureResult(absoluteOf(missing[0]), message)],
      unfinished: [],
      runs: 1,
      crashed: false,
      output: "",
    };
  }
  if (depth >= BISECT_ROUNDS) {
    const message =
      `${HUNG_FAILURE}\n${missing.length} file(s) did not finish together after ${depth} splits\n${tail(run.output)}`.trim();
    return {
      results: [...results, ...missing.map((file) => failureResult(absoluteOf(file), message))],
      unfinished: [],
      runs: 1,
      crashed: false,
      output: "",
    };
  }
  const total = { results, unfinished: [], runs: 1, crashed: false, output: "" };
  const budget = nextBatchTimeout(run.elapsedMs);
  for (const half of chunksOf(missing, Math.ceil(missing.length / 2))) {
    const part = runBatchIsolated(
      suite,
      half,
      { ...ctx, deadline: Math.min(ctx.deadline, Date.now() + budget) },
      depth + 1
    );
    total.results.push(...part.results);
    total.unfinished.push(...part.unfinished);
    total.runs += part.runs;
    total.crashed = total.crashed || part.crashed;
    total.output = total.output || part.output;
  }
  return total;
}

// Runs every file of a suite in bounded batches. `unfinished` holds files the
// suite's deadline never reached; `elapsed` is the wall time it took.
function runSuiteFiles(
  suite,
  {
    sh,
    tmp,
    root,
    host,
    log,
    loadavg = osLoadavg,
    files = null,
    deadline = Date.now() + SUITE_TIMEOUT_MS,
    label = "all",
  }
) {
  // Listing is what names the files a spent budget leaves unrun, so it gets a
  // short allowance even then; it never delays the summary past the run's own.
  const listTimeout = Math.max(1000, Math.min(BATCH_TIMEOUT_MS, deadline - Date.now()));
  const all = files ?? listFiles(suite, { sh, root, timeoutMs: listTimeout });
  if (all === null)
    return {
      results: [],
      unfinished: [],
      error: "vitest list --filesOnly failed",
      crashed: false,
      output: "",
      elapsedMs: 0,
    };
  if (all.length === 0)
    return {
      results: [],
      unfinished: [],
      error: "vitest listed no test files",
      crashed: false,
      output: "",
      elapsedMs: 0,
    };
  const ctx = { sh, tmp, root, host, loadavg, deadline, lastElapsedMs: null, lastWorkers: 0, seq: 0, label };
  const startedAt = Date.now();
  const results = [];
  const unfinished = [];
  let crashed = false;
  let output = "";
  let size = suite.batchFiles ?? BATCH_FILES;
  let pending = all;
  while (pending.length > 0) {
    if (Date.now() >= ctx.deadline) {
      unfinished.push(...pending);
      break;
    }
    const batch = pending.slice(0, size);
    pending = pending.slice(size);
    const part = runBatchIsolated(suite, batch, ctx);
    ctx.seq += part.runs;
    results.push(...part.results);
    unfinished.push(...part.unfinished);
    log(`full-suite: ${suite.name} batch of ${batch.length} took ${Math.round(ctx.lastElapsedMs / 1000)} s`);
    if (part.crashed) {
      crashed = true;
      output = part.output;
    }
    size = nextBatchSize(size, ctx.lastElapsedMs);
  }
  return { results, unfinished, error: null, crashed, output, elapsedMs: Date.now() - startedAt };
}

// One suite's report and crash. A file the deadline never reached is named in
// the crash, not silently dropped: the run is red and the report says which.
function runSuite(suite, ctx) {
  const run = runSuiteFiles(suite, ctx);
  const report = run.error ? null : reportFromFiles(run.results);
  // A suite whose files all passed but whose run still failed (an unhandled
  // rejection, a worker that died between files) has no failing test to show
  // for it: that is a crash, and the tail says why.
  let crash = run.error ? run.error : run.crashed ? run.output : null;
  if (run.unfinished.length > 0) {
    const names = run.unfinished.slice(0, EXCERPT_LINES).join("\n");
    const more = run.unfinished.length > EXCERPT_LINES ? `\n(+${run.unfinished.length - EXCERPT_LINES} more)` : "";
    crash = `suite budget spent: ${run.unfinished.length} file(s) not run\n${names}${more}`;
  }
  return { report, crash, ranFiles: run.results.length, elapsedMs: run.elapsedMs };
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

function runAll(opts, { sh, git, root, tmp, write, log, host, loadavg }) {
  // The budget starts before the install so the whole run, install included,
  // ends inside the scheduler's 45 minutes.
  const startedAt = Date.now();
  const budgetEndsAt = startedAt + TOTAL_BUDGET_MS;
  if (opts.install) {
    const timeoutMs = Math.min(INSTALL_TIMEOUT_MS, budgetEndsAt - Date.now());
    const install = sh("pnpm", ["install", "--frozen-lockfile"], root, { timeoutMs });
    if (install.status !== 0 || install.timedOut) throw new Error(`pnpm install failed:\n${tail(install.output)}`);
  }
  const since = knownSince(opts.since, git, log);
  const summary = {
    head: git(["rev-parse", "HEAD"]),
    green: true,
    // False until every suite has run: a summary written part-way (the run
    // was stopped) must not read as a green run.
    complete: false,
    suites: [],
    range: { since, requestedSince: opts.since, commits: commitLog(git, since) },
  };
  // The head and merge range are worth keeping on disk even if the run is
  // stopped before its first suite finishes.
  if (opts.out) emit({ ...summary, green: false, seconds: 0 }, opts, write);
  for (const suite of SUITES) {
    // Before each suite starts, the file on disk holds every suite finished so
    // far: if the run is stopped later, the scheduler still reads which files
    // ran and failed. `complete` stays false until the last suite is done.
    if (opts.out && summary.suites.length > 0)
      emit({ ...summary, green: false, seconds: Math.round((Date.now() - startedAt) / 1000) }, opts, write);
    log(`full-suite: running ${suite.name}`);
    const first = runSuite(suite, { sh, tmp, root, host, log, loadavg, deadline: suiteDeadline(budgetEndsAt) });
    const entry = {
      name: suite.name,
      counts: countsOf(first.report),
      crash: first.crash,
      rerunCrash: null,
      seconds: Math.round(first.elapsedMs / 1000),
      failures: [],
    };
    const failed = failuresOf(first.report, root);
    if (failed.length > 0) {
      const files = uniqueFiles(failed);
      log(`full-suite: re-running ${files.length} failing ${suite.name} file(s)`);
      // The re-run gets its own deadline, sized from the files it runs, so a
      // failure seen in the last batch still gets a second chance.
      const rerun = runSuiteFiles(suite, {
        sh,
        tmp,
        root,
        host,
        log,
        loadavg,
        files: files.map((file) => suiteRelative(suite, file)),
        deadline: rerunDeadline(files.length, budgetEndsAt),
        label: "rerun",
      });
      // The re-run is given its files, so it cannot fail to list them.
      const rerunReport = reportFromFiles(rerun.results);
      entry.rerunCrash =
        (rerun.crashed ? rerun.output : null) ??
        (rerun.unfinished.length > 0 ? `re-run did not run ${rerun.unfinished.length} file(s)` : null);
      const commits = new Map(files.map((file) => [file, commitLog(git, since, file)]));
      entry.failures = classify(failed, failuresOf(rerunReport, root), entry.rerunCrash !== null).map((failure) => ({
        ...failure,
        commits: commits.get(failure.file),
      }));
    }
    if (entry.crash || entry.failures.some((failure) => failure.kind === "real")) summary.green = false;
    summary.suites.push(entry);
    log(
      `full-suite: ${entry.name} ${entry.counts.files} files, ${entry.counts.testsFailed} failed in ${entry.seconds} s`
    );
  }
  summary.seconds = Math.round((Date.now() - startedAt) / 1000);
  summary.complete = true;
  emit(summary, opts, write);
  log(`full-suite: ${summary.head.slice(0, 9)} ${summary.green ? "green" : "red"} in ${summary.seconds} s`);
  return summary.green ? 0 : 1;
}

// A re-run only has a handful of files, so it gets a small share of the batch
// allowance — enough for a slow file list twice over, not the whole budget.
function nextRerunTimeout(fileCount) {
  return Math.min(MAX_BATCH_TIMEOUT_MS, Math.max(MIN_BATCH_TIMEOUT_MS, fileCount * 30_000));
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
    loadavg = osLoadavg,
  } = {}
) {
  let opts = { out: null };
  let status;
  const dir = tmp ?? mkdtempSync(join(tmpdir(), "full-suite-"));
  try {
    opts = parseArgs(argv);
    status = runAll(opts, { sh, git, root, tmp: dir, write, log, host, loadavg });
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

export {
  classify,
  countsOf,
  excerpt,
  failuresOf,
  filesOf,
  isCliEntry,
  main,
  nextBatchSize,
  nextBatchTimeout,
  parseArgs,
  progressOf,
  progressResult,
  reportFromFiles,
  rerunDeadline,
  runBatchIsolated,
  runSuite,
  runSuiteFiles,
  suiteDeadline,
  suiteRelative,
  uniqueFiles,
  workerArgs,
  workerCount,
};
