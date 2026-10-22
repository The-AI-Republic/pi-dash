// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs the web_new unit tests a change can affect, not the whole suite.
// Vitest's --changed follows the import graph from the files that differ from
// the base (including @pidash/kit and @pidash/api-client sources, which
// vitest.config.ts aliases in). A few inputs reach every test without being
// imported (test setup, configs, dependencies); touching one of those runs the
// whole suite instead. Agents run this locally (`pnpm --filter web_new
// test:changed`), CI runs it on PRs, and pushes to web-new-dev still run the
// full suite.
//
// Usage: node checks/test-changed.mjs [--base <ref>] [--full] [vitest args...]
//   --base <ref>  diff against this ref (default: merge base of HEAD and
//                 origin/web-new-dev)
//   --full        run the whole suite regardless of what changed
// Any other argument (e.g. --coverage) is passed through to vitest.
import { execFileSync, spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const webNewDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_BASE_BRANCH = "origin/web-new-dev";

// Repo-relative paths whose change can affect any test without an import edge.
const FULL_SUITE_PATHS = [
  /^pnpm-lock\.yaml$/,
  /^package\.json$/,
  /^(apps\/web_new|packages\/kit|packages\/api-client)\/package\.json$/,
  /^apps\/web_new\/(vitest|vite)\.config\.ts$/,
  /^apps\/web_new\/tsconfig\.json$/,
  /^apps\/web_new\/src\/test-setup\.ts$/,
];

function fullSuiteReasons(files) {
  return files.filter((file) => FULL_SUITE_PATHS.some((pattern) => pattern.test(file)));
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
    else opts.vitestArgs.push(arg);
  }
  return opts;
}

// Decide what to run. Pure, so the self-test covers every branch.
function planRun({ base, full, vitestArgs, changedFiles }) {
  if (full) return { mode: "full", reasons: ["--full"], args: ["run", ...vitestArgs] };
  const reasons = fullSuiteReasons(changedFiles);
  if (reasons.length > 0) return { mode: "full", reasons, args: ["run", ...vitestArgs] };
  return { mode: "changed", reasons: [], args: ["run", "--changed", base, "--passWithNoTests", ...vitestArgs] };
}

function defaultGit(args) {
  return execFileSync("git", args, { cwd: webNewDir, encoding: "utf8" }).trim();
}

/* v8 ignore next 3 -- spawns vitest itself; the self-test injects a fake */
function defaultRunVitest(args) {
  return spawnSync("pnpm", ["exec", "vitest", ...args], { cwd: webNewDir, stdio: "inherit" }).status ?? 1;
}

// Files that differ from base: committed, staged, unstaged and untracked.
function changedFilesSince(base, git) {
  const root = git(["rev-parse", "--show-toplevel"]);
  const tracked = git(["-C", root, "diff", "--name-only", base]);
  const untracked = git(["-C", root, "ls-files", "--others", "--exclude-standard"]);
  return [...new Set(`${tracked}\n${untracked}`.split("\n").filter(Boolean))];
}

function main(argv = process.argv.slice(2), { git = defaultGit, runVitest = defaultRunVitest, log = console.log } = {}) {
  const opts = parseArgs(argv);
  const base = opts.base ?? git(["merge-base", "HEAD", DEFAULT_BASE_BRANCH]);
  const changedFiles = opts.full ? [] : changedFilesSince(base, git);
  const plan = planRun({ ...opts, base, changedFiles });
  if (plan.mode === "full") log(`test:changed: full suite (${plan.reasons.join(", ")})`);
  else log(`test:changed: tests affected by ${changedFiles.length} file(s) changed since ${base}`);
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

export { changedFilesSince, fullSuiteReasons, isCliEntry, main, parseArgs, planRun };
