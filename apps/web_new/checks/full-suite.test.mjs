// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the scheduled full-suite runner: how vitest reports become
// failures and crashes, how re-runs split real from flaky, and what the
// summary and exit code say.
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  classify,
  countsOf,
  excerpt,
  failuresOf,
  isCliEntry,
  main,
  parseArgs,
  uniqueFiles,
  workerArgs,
} from "./full-suite.mjs";

const ROOT = "/repo";
const LOAD = "(file failed to load)";
const WEB_WORKERS = "--maxWorkers=16";
const SCHEDULER_HOST = { cpus: 4, memoryGiB: 30 };

function report(files, extra = {}) {
  const tests = files.flatMap((file) => file.assertionResults ?? []);
  return {
    // Deliberately unlike the file count: vitest counts describe blocks here.
    numTotalTestSuites: files.length + 10,
    numTotalTests: tests.length,
    numFailedTests: tests.filter((test) => test.status === "failed").length,
    success: files.every((file) => file.status !== "failed"),
    testResults: files,
    ...extra,
  };
}

const passing = {
  name: `${ROOT}/apps/web_new/src/a.test.ts`,
  status: "passed",
  assertionResults: [{ fullName: "a works", status: "passed" }],
};
const failing = {
  name: `${ROOT}/apps/web_new/src/b.test.ts`,
  status: "failed",
  assertionResults: [
    { fullName: "b ok", status: "passed", failureMessages: [] },
    {
      fullName: "b breaks",
      status: "failed",
      failureMessages: ["AssertionError: expected 1 to be 2", "at b.test.ts:3"],
    },
  ],
};
const broken = { name: `${ROOT}/apps/web_new/src/c.test.ts`, status: "failed", message: "Cannot find module './gone'" };

describe("parseArgs", () => {
  it("reads --since, --out and --no-install", () => {
    expect(parseArgs(["--since", "abc", "--out", "s.json", "--no-install"])).toEqual({
      since: "abc",
      out: "s.json",
      install: false,
    });
    expect(parseArgs([])).toEqual({ since: null, out: null, install: true });
  });

  it("refuses a flag without a value and unknown arguments", () => {
    expect(() => parseArgs(["--since"])).toThrow("--since needs a value");
    expect(() => parseArgs(["--full"])).toThrow("unknown argument --full");
  });
});

describe("workerArgs", () => {
  it("oversubscribes a small CPU pool when memory has room", () => {
    expect(workerArgs(SCHEDULER_HOST)).toEqual([WEB_WORKERS]);
    expect(workerArgs({ cpus: 4, memoryGiB: 12 })).toEqual(["--maxWorkers=8"]);
    expect(workerArgs({ cpus: 2, memoryGiB: 24 })).toEqual([WEB_WORKERS]);
  });

  it("leaves vitest's default pool alone when memory is the constraint", () => {
    expect(workerArgs({ cpus: 8, memoryGiB: 8 })).toEqual([]);
    expect(workerArgs({ cpus: 4, memoryGiB: 4 })).toEqual([]);
    expect(workerArgs({ cpus: 16, memoryGiB: 64 })).toEqual([]);
  });
});

describe("excerpt", () => {
  it("keeps the first 30 lines, trimmed", () => {
    const text = Array.from({ length: 40 }, (_, i) => `line ${i}`).join("\n");
    expect(excerpt(text).split("\n")).toHaveLength(30);
    expect(excerpt("  one\n")).toBe("one");
  });
});

describe("failuresOf", () => {
  it("lists failing tests and files that failed to load, with repo-relative paths", () => {
    expect(failuresOf(report([passing, failing, broken]), ROOT)).toEqual([
      {
        file: "apps/web_new/src/b.test.ts",
        test: "b breaks",
        error: "AssertionError: expected 1 to be 2\nat b.test.ts:3",
      },
      { file: "apps/web_new/src/c.test.ts", test: LOAD, error: "Cannot find module './gone'" },
    ]);
  });

  it("copes with a missing report or missing fields", () => {
    expect(failuresOf(null, ROOT)).toEqual([]);
    expect(failuresOf({ testResults: [{ name: `${ROOT}/x.test.ts`, status: "failed" }] }, ROOT)).toEqual([
      { file: "x.test.ts", test: LOAD, error: "" },
    ]);
    const noMessages = {
      name: `${ROOT}/y.test.ts`,
      status: "failed",
      assertionResults: [{ fullName: "t", status: "failed" }],
    };
    expect(failuresOf({ testResults: [noMessages] }, ROOT)).toEqual([{ file: "y.test.ts", test: "t", error: "" }]);
  });
});

describe("countsOf", () => {
  it("counts files from testResults and tests from vitest's totals", () => {
    expect(countsOf(report([passing, failing]))).toEqual({ files: 2, filesFailed: 1, tests: 3, testsFailed: 1 });
    expect(countsOf(null)).toEqual({ files: 0, filesFailed: 0, tests: 0, testsFailed: 0 });
    expect(countsOf({ numTotalTests: 0, numFailedTests: 0 })).toEqual({
      files: 0,
      filesFailed: 0,
      tests: 0,
      testsFailed: 0,
    });
  });
});

describe("classify and uniqueFiles", () => {
  const first = [
    { file: "f.test.ts", test: "one" },
    { file: "f.test.ts", test: "two" },
  ];

  it("marks a failure real when the same test fails again and flaky when it passes", () => {
    expect(classify(first, [{ file: "f.test.ts", test: "two" }]).map((failure) => failure.kind)).toEqual([
      "flaky",
      "real",
    ]);
    expect(uniqueFiles(first)).toEqual(["f.test.ts"]);
  });

  it("matches load failures by file in either direction", () => {
    expect(classify([{ file: "g.test.ts", test: LOAD }], [{ file: "g.test.ts", test: "g breaks" }])[0].kind).toBe(
      "real"
    );
    expect(classify(first, [{ file: "f.test.ts", test: LOAD }]).map((failure) => failure.kind)).toEqual([
      "real",
      "real",
    ]);
    expect(classify([{ file: "g.test.ts", test: LOAD }], [{ file: "other.test.ts", test: "x" }])[0].kind).toBe("flaky");
  });

  it("keeps every failure real when the re-run crashed", () => {
    expect(classify(first, [], true).map((failure) => failure.kind)).toEqual(["real", "real"]);
  });
});

// A fake shell. `runs` maps "<suite dir>:<all|rerun>" to what that vitest run
// does: { report, status, timedOut, output }. A run with no report writes none.
function fakeShell({ runs = {}, install = { status: 0 } } = {}) {
  const calls = [];
  const sh = (cmd, args, cwd, opts) => {
    calls.push({ cmd, args, cwd, opts });
    if (args[0] === "install") return { status: 0, output: "", timedOut: false, ...install };
    const flag = args.findIndex((arg) => arg.startsWith("--outputFile="));
    const outFile = args[flag].slice("--outputFile=".length);
    // A re-run passes failing files after the output file; option values and
    // the suite's worker flags are not file arguments.
    const key = `${cwd.slice(ROOT.length + 1)}:${args.slice(flag + 1).some((arg) => arg.endsWith(".test.tsx") || arg.endsWith(".test.ts") || arg.endsWith(".test.mjs")) ? "rerun" : "all"}`;
    const run = runs[key] ?? {};
    if (run.report) writeFileSync(outFile, JSON.stringify(run.report));
    const status = run.status ?? (run.report ? (run.report.success === false ? 1 : 0) : 1);
    return { status, output: run.output ?? "out\nSegmentation fault", timedOut: run.timedOut ?? false };
  };
  return { sh, calls };
}

function fakeGit({ known = true } = {}) {
  const calls = [];
  const git = (args) => {
    calls.push(args.join(" "));
    if (args.includes("--verify")) {
      if (!known) throw new Error("fatal: bad revision");
      return "abc123full";
    }
    if (args[0] === "rev-parse") return "0123456789abcdef";
    if (args.includes("--")) return args.at(-1).includes("b.test") ? "c0ffee1 NEWFRONT-9 change b" : "";
    return "c0ffee1 NEWFRONT-9 change b\nbeef002 NEWFRONT-8 other";
  };
  return { git, calls };
}

const green = {
  "apps/web_new:all": { report: report([passing]) },
  "packages/kit:all": { report: report([]) },
  "packages/api-client:all": { report: report([]) },
};

function run(argv, { runs = green, install, git = fakeGit() } = {}) {
  const tmp = mkdtempSync(join(tmpdir(), "full-suite-test-"));
  const shell = fakeShell({ runs, install });
  const written = {};
  const logs = [];
  const status = main(argv, {
    sh: shell.sh,
    git: git.git,
    root: ROOT,
    tmp,
    host: SCHEDULER_HOST,
    write: (file, text) => (written[file] = JSON.parse(text)),
    log: (line) => logs.push(line),
  });
  return { status, summary: written["s.json"], shell, git, logs, tmp };
}

describe("main", () => {
  afterEach(() => vi.restoreAllMocks());

  it("installs, runs all three suites with a timeout and reports green", () => {
    const { status, summary, shell, logs, tmp } = run(["--out", "s.json"]);
    expect(status).toBe(0);
    expect(shell.calls[0]).toMatchObject({ cmd: "pnpm", args: ["install", "--frozen-lockfile"], cwd: ROOT });
    expect(shell.calls.slice(1).map((call) => call.cwd)).toEqual([
      `${ROOT}/apps/web_new`,
      `${ROOT}/packages/kit`,
      `${ROOT}/packages/api-client`,
    ]);
    // Only web_new gets the larger pool; kit and api-client keep vitest's default.
    expect(shell.calls[1].args).toContain(WEB_WORKERS);
    expect(shell.calls[2].args).not.toContain(WEB_WORKERS);
    expect(shell.calls[3].args).not.toContain(WEB_WORKERS);
    // Both reporters: JSON for the summary, default so a killed run still has
    // the progress tail that names the in-flight file.
    expect(shell.calls[1].args).toEqual(expect.arrayContaining(["--reporter=default", "--reporter=json"]));
    expect(shell.calls[1].opts.timeoutMs).toBe(45 * 60 * 1000);
    expect(summary.head).toBe("0123456789abcdef");
    expect(summary.green).toBe(true);
    expect(summary.range).toEqual({
      since: null,
      requestedSince: null,
      commits: ["c0ffee1 NEWFRONT-9 change b", "beef002 NEWFRONT-8 other"],
    });
    expect(summary.suites[0]).toEqual({
      name: "web_new",
      counts: { files: 1, filesFailed: 0, tests: 1, testsFailed: 0 },
      crash: null,
      rerunCrash: null,
      failures: [],
    });
    expect(logs.at(-1)).toBe("full-suite: 012345678 green");
    expect(existsSync(tmp)).toBe(false);
  });

  it("re-runs failing files once, splits real from flaky and looks up commits once per file", () => {
    const twoFailures = {
      ...failing,
      assertionResults: [...failing.assertionResults, { fullName: "b also", status: "failed" }],
    };
    const runs = {
      ...green,
      "apps/web_new:all": { report: report([passing, twoFailures, broken]) },
      "apps/web_new:rerun": { report: report([twoFailures, { ...broken, status: "passed", message: "" }]) },
    };
    const { status, summary, shell, git } = run(["--out", "s.json", "--since", "abc123", "--no-install"], { runs });
    expect(status).toBe(1);
    expect(shell.calls[0].args.slice(0, 3)).toEqual(["exec", "vitest", "run"]);
    expect(shell.calls[1].args.slice(-2)).toEqual([
      `${ROOT}/apps/web_new/src/b.test.ts`,
      `${ROOT}/apps/web_new/src/c.test.ts`,
    ]);
    expect(summary.green).toBe(false);
    expect(summary.range.since).toBe("abc123");
    expect(git.calls).toContain("log --format=%h %s abc123..HEAD");
    expect(git.calls.filter((call) => call.endsWith("-- apps/web_new/src/b.test.ts"))).toHaveLength(1);
    expect(summary.suites[0].failures.map(({ test, kind, commits }) => ({ test, kind, commits }))).toEqual([
      { test: "b breaks", kind: "real", commits: ["c0ffee1 NEWFRONT-9 change b"] },
      { test: "b also", kind: "real", commits: ["c0ffee1 NEWFRONT-9 change b"] },
      { test: LOAD, kind: "flaky", commits: [] },
    ]);
  });

  it("stays green when every failure was flaky", () => {
    const runs = {
      ...green,
      "packages/kit:all": { report: report([failing]) },
      "packages/kit:rerun": { report: report([]) },
    };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { runs });
    expect(status).toBe(0);
    expect(summary.suites[1].failures.map((failure) => failure.kind)).toEqual(["flaky"]);
  });

  it("keeps failures real and goes red when the re-run crashes", () => {
    const runs = { ...green, "apps/web_new:all": { report: report([failing]) } };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { runs });
    expect(status).toBe(1);
    expect(summary.suites[0].rerunCrash).toBe("out\nSegmentation fault");
    expect(summary.suites[0].failures[0].kind).toBe("real");
  });

  it("reports a suite that wrote no report as a crash", () => {
    const runs = { "apps/web_new:all": green["apps/web_new:all"], "packages/kit:all": green["packages/kit:all"] };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { runs });
    expect(status).toBe(1);
    expect(summary.suites[2]).toEqual({
      name: "api-client",
      counts: { files: 0, filesFailed: 0, tests: 0, testsFailed: 0 },
      crash: "out\nSegmentation fault",
      rerunCrash: null,
      failures: [],
    });
  });

  it("reports a failed run with no failing test as a crash", () => {
    const unhandled = { report: report([passing], { success: false }), status: 1, output: "Unhandled Rejection\nboom" };
    const noFiles = { report: report([]), status: 1, output: "No test files found" };
    const runs = { ...green, "apps/web_new:all": unhandled, "packages/kit:all": noFiles };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { runs });
    expect(status).toBe(1);
    expect(summary.suites[0].crash).toBe("Unhandled Rejection\nboom");
    expect(summary.suites[1].crash).toBe("No test files found");
  });

  it("reports a suite that timed out as a crash", () => {
    const runs = { ...green, "apps/web_new:all": { timedOut: true, output: "still running" } };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { runs });
    expect(status).toBe(1);
    expect(summary.suites[0].crash).toBe("timed out after 45 min\nstill running");
  });

  it("falls back to the last 12 hours when --since is not a known commit", () => {
    const { status, summary, logs } = run(["--out", "s.json", "--no-install", "--since", "gone"], {
      git: fakeGit({ known: false }),
    });
    expect(status).toBe(0);
    expect(summary.range).toMatchObject({ since: null, requestedSince: "gone" });
    expect(logs).toContain("full-suite: --since gone is not a known commit; using the last 12 hours");
  });

  it("exits 2 with an error summary when the install fails or times out", () => {
    const failed = run(["--out", "s.json"], { install: { status: 1, output: "ERR lockfile out of date" } });
    expect(failed.status).toBe(2);
    expect(failed.summary).toEqual({ error: "pnpm install failed:\nERR lockfile out of date" });
    expect(run(["--out", "s.json"], { install: { timedOut: true, output: "hung" } }).summary.error).toBe(
      "pnpm install failed:\nhung"
    );
  });

  it("exits 2 on a bad argument, printing the error to stdout and removing its own temp folder", () => {
    const writes = [];
    vi.spyOn(process.stdout, "write").mockImplementation((text) => (writes.push(text), true));
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const status = main(["--bogus"]);
    expect(status).toBe(2);
    expect(JSON.parse(writes.join(""))).toEqual({ error: "unknown argument --bogus" });
    expect(errors).toHaveBeenCalledWith("full-suite: error: unknown argument --bogus");
  });

  it("prints the summary to stdout without --out", () => {
    const writes = [];
    vi.spyOn(process.stdout, "write").mockImplementation((text) => (writes.push(text), true));
    const { status } = run(["--no-install"]);
    expect(status).toBe(0);
    expect(JSON.parse(writes.join("")).green).toBe(true);
  });

  it("handles an empty merge range", () => {
    const git = { git: (args) => (args[0] === "rev-parse" ? "fedcba987654" : "") };
    expect(run(["--out", "s.json", "--no-install"], { git }).summary.range.commits).toEqual([]);
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
