// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs the web_new unit tests a change can affect, not the whole suite.
// test-impact.mjs picks them by imported name from the files that differ from
// the base (web_new plus @pidash/kit and @pidash/api-client sources), so an
// entry file that re-exports a whole feature or package does not drag in every
// test that touches it; vitest then runs exactly those files. Configs, package
// manifests and the test setup file reach tests without an import, so they
// select nothing (the output says so). The NEWFRONT full-suite scheduler runs
// the whole suite on web-new-dev every 12 hours and covers them. Agents run
// this locally (`pnpm --filter web_new test:changed`) and CI runs it on PRs.
//
// Usage: node checks/test-changed.mjs [--base <ref>] [--full] [vitest args...]
//   --base <ref>  diff against this ref (default: merge base of HEAD and
//                 origin/web-new-dev)
//   --full        run the whole suite regardless of what changed
// Any other option (e.g. --coverage) is passed through to vitest. Path
// filters are refused: the selection is the filter. Past MAX_TEST_FILES
// selected files the list could outgrow the OS argument limit, so the whole
// suite runs. The fork pool is sized from the host's sustained load the way
// the full-suite runner sizes it (see workerCount in full-suite.mjs); an
// explicit --maxWorkers passes through untouched and disables the sizing.
import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { availableParallelism, loadavg as osLoadavg, totalmem } from "node:os";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { resolveEditionModule } from "../tools/edition-module.mjs";
import { workerArgs, workerCount } from "./full-suite.mjs";
import { SOURCE_ROOTS, affectedTests, aliasesFor } from "./test-impact.mjs";

const webNewDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_BASE_BRANCH = "origin/web-new-dev";
const MAX_TEST_FILES = 1000;

// Repo-relative paths that reach tests without an import edge. They select
// no tests; the output names them so a reader knows they went untested.
const NO_IMPORT_EDGE_PATHS = [
  /(^|\/)package\.json$/,
  /(^|\/)pnpm-lock\.yaml$/,
  /(^|\/)(vitest|vite)\.config\.[cm]?[jt]s$/,
  /(^|\/)tsconfig(\.[^/]*)?\.json$/,
  /^apps\/web_new\/src\/test-setup\.ts$/,
];

function skippedFiles(files) {
  return files.filter((file) => NO_IMPORT_EDGE_PATHS.some((pattern) => pattern.test(file)));
}

function parseArgs(argv) {
  const opts = { base: null, full: false, vitestArgs: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--full") opts.full = true;
    else if (arg === "--base") {
      opts.base = argv[i + 1];
      i += 1;
      if (!opts.base) throw new Error("--base needs a ref");
    } else if (arg.startsWith("--base=")) opts.base = arg.slice("--base=".length);
    else {
      // A bare word is an option's value (`--reporter json`) or a path filter.
      const previous = opts.vitestArgs.at(-1);
      const isValue = previous !== undefined && previous.startsWith("--") && !previous.includes("=");
      if (!arg.startsWith("-") && !isValue)
        throw new Error(`test:changed takes no path filters (${arg}); run pnpm exec vitest run <files> instead`);
      opts.vitestArgs.push(arg);
    }
  }
  return opts;
}

// Vitest spellings of the worker-pool flag. When the user passes one
// explicitly it wins and no sized flag is added.
const WORKER_FLAGS = ["--maxWorkers", "--max-workers"];

function hasWorkerFlag(vitestArgs) {
  return vitestArgs.some((arg) => WORKER_FLAGS.some((flag) => arg === flag || arg.startsWith(`${flag}=`)));
}

// Decide what to run from the selected test files. Pure, so the self-test
// covers every branch. Paths are repo-relative; vitest gets them absolute.
// `pool` is the sized worker-pool flag (empty when it matches vitest's
// default or the user passed their own); it precedes the passthrough args so
// a user spelling this file does not know still wins by position.
function planRun({ full, vitestArgs, tests, root, pool = [] }) {
  if (full) return { mode: "full", reason: "--full", args: ["run", ...pool, ...vitestArgs] };
  if (tests.length > MAX_TEST_FILES)
    return { mode: "full", reason: `${tests.length} test files selected`, args: ["run", ...pool, ...vitestArgs] };
  if (tests.length === 0) return { mode: "none", reason: null, args: null };
  return {
    mode: "changed",
    reason: null,
    args: ["run", "--passWithNoTests", ...pool, ...vitestArgs, ...tests.map((file) => resolve(root, file))],
  };
}

function defaultGit(args) {
  return execFileSync("git", args, {
    cwd: webNewDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 64 * 1024 * 1024,
  }).trim();
}

function defaultRead(root) {
  return (file) => {
    try {
      return readFileSync(resolve(root, file), "utf8");
    } catch {
      return null;
    }
  };
}

// Every tracked or new file the graph needs, and each file's text at the base.
// Each file's text at the base (null where it did not exist), from one
// `git cat-file --batch` instead of a git process per file.
function defaultReadBase(root, base, files) {
  const texts = new Map();
  if (files.length === 0) return texts;
  const input = files.map((file) => `${base}:${file}\n`).join("");
  const result = spawnSync("git", ["-C", root, "cat-file", "--batch"], { input, maxBuffer: 512 * 1024 * 1024 });
  if (result.status !== 0) return texts;
  const out = result.stdout;
  let at = 0;
  for (const file of files) {
    const end = out.indexOf(10, at);
    const header = out.toString("utf8", at, end).split(" ");
    at = end + 1;
    if (header[1] !== "blob") {
      texts.set(file, null);
      continue;
    }
    const size = Number(header[2]);
    texts.set(file, out.toString("utf8", at, at + size));
    at += size + 1;
  }
  return texts;
}

// The graph needs every tracked or new file under the source roots, each
// changed file's text at the base, and the edition module vitest loads.
function selectTests(changedFiles, { base, root, git, read, readBaseFiles, env }) {
  const files = git(["-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", ...SOURCE_ROOTS])
    .split("\n")
    .filter(Boolean);
  const baseTexts = readBaseFiles(root, base, changedFiles);
  const edition = relative(root, resolveEditionModule(env, resolve(root, "apps/web_new")).modulePath);
  return affectedTests({
    changedFiles,
    read,
    readBase: (file) => baseTexts.get(file) ?? null,
    files,
    aliases: aliasesFor(edition),
  });
}

/* v8 ignore next 3 -- spawns vitest itself; the self-test injects a fake */
function defaultRunVitest(args) {
  return spawnSync("pnpm", ["exec", "vitest", ...args], { cwd: webNewDir, stdio: "inherit" }).status ?? 1;
}

// Files that differ from base: committed, staged, unstaged and untracked.
function changedFilesSince(base, git, root = git(["rev-parse", "--show-toplevel"])) {
  const tracked = git(["-C", root, "diff", "--name-only", base]);
  const untracked = git(["-C", root, "ls-files", "--others", "--exclude-standard"]);
  return [...new Set(`${tracked}\n${untracked}`.split("\n").filter(Boolean))];
}

function main(
  argv = process.argv.slice(2),
  {
    git = defaultGit,
    runVitest = defaultRunVitest,
    log = console.log,
    read = null,
    readBaseFiles = defaultReadBase,
    env = process.env,
    host = { cpus: availableParallelism(), memoryGiB: totalmem() / 1024 ** 3 },
    loadavg = osLoadavg,
  } = {}
) {
  const opts = parseArgs(argv);
  // One vitest invocation, so the whole 5-minute average is load outside this
  // run (the full-suite runner subtracts its previous batch; there is none
  // here). An explicit user flag disables the sizing.
  const externalLoad = loadavg()[1];
  const workers = workerCount({ ...host, externalLoad });
  const pool = hasWorkerFlag(opts.vitestArgs) ? [] : workerArgs({ ...host, externalLoad });
  const poolNote =
    pool.length > 0
      ? `test:changed: ${workers} workers (--maxWorkers=${workers}; 5-min load ${externalLoad.toFixed(1)})`
      : null;
  if (opts.full) {
    log("test:changed: full suite (--full)");
    if (poolNote) log(poolNote);
    return runVitest(planRun({ ...opts, tests: [], root: "", pool }).args);
  }
  const base = opts.base ?? git(["merge-base", "HEAD", DEFAULT_BASE_BRANCH]);
  const root = git(["rev-parse", "--show-toplevel"]);
  const changedFiles = changedFilesSince(base, git, root);
  const { tests, setupReached } = selectTests(changedFiles, {
    base,
    root,
    git,
    read: read ?? defaultRead(root),
    readBaseFiles,
    env,
  });
  const plan = planRun({ ...opts, tests, root, pool });
  const skipped = skippedFiles(changedFiles);
  if (plan.mode === "full") log(`test:changed: full suite (${plan.reason} since ${base})`);
  else log(`test:changed: ${changedFiles.length} file(s) changed since ${base} reach ${tests.length} test file(s)`);
  if (poolNote) log(poolNote);
  if (skipped.length > 0) log(`test:changed: no import edge, left to the scheduled full suite: ${skipped.join(", ")}`);
  if (setupReached)
    log("test:changed: the change reaches src/test-setup.ts, which every test loads; left to the scheduled full suite");
  if (plan.mode === "none") return 0;
  return runVitest(plan.args);
}

// True when this module is the process entry point. Exported so the self-test
// can cover it without relying on a child process's coverage.
function isCliEntry(argv1 = process.argv[1]) {
  return argv1 !== undefined && import.meta.url === pathToFileURL(argv1).href;
}

// Stay a pure module when imported (e.g. by the test). When this file is the
// process entry point, exit with vitest's status.
/* v8 ignore next 3 -- child-process bootstrap; CI and agents run it */
if (isCliEntry()) {
  process.exit(main());
}

export { changedFilesSince, defaultReadBase, hasWorkerFlag, isCliEntry, main, parseArgs, planRun, skippedFiles };
