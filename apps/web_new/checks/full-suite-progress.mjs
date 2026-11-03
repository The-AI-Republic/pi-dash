// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A vitest reporter that appends one JSON line per test module to the file
// named by FULL_SUITE_PROGRESS. The stock JSON reporter only writes after the
// run ends, so a suite stopped by `checks/full-suite.mjs`'s timeout left no
// per-file evidence at all (`files=0`). This stream names every file that
// finished and every file still running when the run was stopped, which is
// what turns a timeout into a report someone can act on.
//
// The runner passes the path through the environment because vitest's CLI has
// no option channel for a custom reporter's own settings.
import { appendFileSync } from "node:fs";

const PROGRESS_ENV = "FULL_SUITE_PROGRESS";

function line(event) {
  return `${JSON.stringify(event)}\n`;
}

function append(path, event) {
  if (!path) return;
  appendFileSync(path, line(event));
}

// A test case's full name: its describe titles from the outside in, then the
// test's own name — the same shape vitest's JSON report uses.
function fullNameOf(test) {
  const titles = [];
  let node = test.suite;
  while (node) {
    if (node.name) titles.unshift(node.name);
    node = node.suite;
  }
  titles.push(test.name);
  return titles.join(" > ");
}

function messageOf(test) {
  const errors = test.result?.errors ?? [];
  const text = errors.map((error) => error.stack ?? error.message ?? "").join("\n");
  return String(text).split("\n").slice(0, 5).join("\n");
}

// Walks a module's test cases. Suites nest, so this recurses rather than
// relying on a per-module aggregate vitest does not expose on the reporter's
// task. Failed tests keep their names and messages, so a report recovered from
// these lines can be re-run and classified exactly like the JSON report's.
function collectTests(task, out = { total: 0, passed: 0, failed: [], skipped: 0 }) {
  for (const child of task?.tasks ?? []) {
    if (child.type === "test" || child.type === "custom") {
      out.total += 1;
      if (child.result?.state === "fail") out.failed.push({ name: fullNameOf(child), message: messageOf(child) });
      else if (child.result?.state === "pass") out.passed += 1;
      else out.skipped += 1;
    } else {
      collectTests(child, out);
    }
  }
  return out;
}

function firstMessage(module) {
  const errors = module?.errors ?? [];
  const error = errors[0];
  if (!error) return "";
  const text = error.stack ?? error.message ?? "";
  return String(text).split("\n").slice(0, 3).join("\n");
}

export default class FullSuiteProgressReporter {
  constructor(options = {}) {
    this.path = options.path ?? process.env[PROGRESS_ENV] ?? "";
  }

  onTestModuleStart(module) {
    append(this.path, { event: "start", file: module.moduleId ?? module.task?.name ?? "" });
  }

  onTestModuleEnd(module) {
    const file = module.moduleId ?? module.task?.name ?? "";
    const tests = collectTests(module.task);
    const failed = module.result?.state === "fail" || tests.failed.length > 0;
    append(this.path, {
      event: "end",
      file,
      state: failed ? "failed" : "passed",
      tests: tests.total,
      passed: tests.passed,
      skipped: tests.skipped,
      failures: tests.failed,
      message: failed ? firstMessage(module) : "",
    });
  }
}

export { PROGRESS_ENV, collectTests, fullNameOf, line };
