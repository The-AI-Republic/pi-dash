// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs the web_new unit tests a change can affect, not the whole suite.
// `vitest related` follows the import graph from the files that differ from
// the base (including @pidash/kit and @pidash/api-client sources, which
// vitest.config.ts aliases in). Configs, package manifests and the test setup
// file reach every test without an import edge; vitest would answer a change
// to one of them by running the whole suite, so they are left out of the list
// and do not widen the run. The full suite on every push to web-new-dev
// covers them. Agents run this locally (`pnpm --filter web_new
// test:changed`) and CI runs it on PRs.
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

// Repo-relative paths that reach tests without an import edge. Vitest's
// forceRerunTriggers (package.json, vite/vitest configs, setup files) would
// turn any of them into a whole-suite run, so they are not passed on.
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
    else opts.vitestArgs.push(arg);
  }
  return opts;
}

// Decide what to run. Pure, so the self-test covers every branch. Paths are
// repo-relative; vitest gets them absolute, as its import graph holds them.
function planRun({ full, vitestArgs, changedFiles, root }) {
  if (full) return { mode: "full", skipped: [], args: ["run", ...vitestArgs] };
  const skipped = skippedFiles(changedFiles);
  const related = changedFiles.filter((file) => !skipped.includes(file)).map((file) => resolve(root, file));
  return { mode: "changed", skipped, args: ["related", "--run", "--passWithNoTests", ...vitestArgs, ...related] };
}

function defaultGit(args) {
  return execFileSync("git", args, { cwd: webNewDir, encoding: "utf8" }).trim();
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
  { git = defaultGit, runVitest = defaultRunVitest, log = console.log } = {}
) {
  const opts = parseArgs(argv);
  if (opts.full) {
    log("test:changed: full suite (--full)");
    return runVitest(planRun({ ...opts, changedFiles: [], root: "" }).args);
  }
  const base = opts.base ?? git(["merge-base", "HEAD", DEFAULT_BASE_BRANCH]);
  const root = git(["rev-parse", "--show-toplevel"]);
  const changedFiles = changedFilesSince(base, git, root);
  const plan = planRun({ ...opts, changedFiles, root });
  log(`test:changed: tests affected by ${changedFiles.length} file(s) changed since ${base}`);
  if (plan.skipped.length > 0) log(`test:changed: no import edge, left to CI's full suite: ${plan.skipped.join(", ")}`);
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

export { changedFilesSince, isCliEntry, main, parseArgs, planRun, skippedFiles };
