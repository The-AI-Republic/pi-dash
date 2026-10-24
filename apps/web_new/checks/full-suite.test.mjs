// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the scheduled full-suite runner: how vitest reports become
// failures, how re-runs split real from flaky, and what the summary holds.
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { classify, countsOf, excerpt, failuresOf, isCliEntry, main, parseArgs, uniqueFiles } from "./full-suite.mjs";

const ROOT = "/repo";

function report(files) {
  const failed = files.filter((file) => file.status === "failed").length;
  const tests = files.flatMap((file) => file.assertionResults ?? []);
  return {
    // Deliberately unlike the file counts: these count describe blocks.
    numTotalTestSuites: files.length + 10,
    numFailedTestSuites: failed,
    numTotalTests: tests.length,
    numFailedTests: tests.filter((test) => test.status === "failed").length,
    testResults: files,
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
      { file: "apps/web_new/src/c.test.ts", test: "(file failed to load)", error: "Cannot find module './gone'" },
    ]);
  });

  it("copes with a missing report or missing fields", () => {
    expect(failuresOf(null, ROOT)).toEqual([]);
    expect(failuresOf({ testResults: [{ name: `${ROOT}/x.test.ts`, status: "failed" }] }, ROOT)).toEqual([
      { file: "x.test.ts", test: "(file failed to load)", error: "" },
    ]);
    expect(
      failuresOf(
        {
          testResults: [
            { name: `${ROOT}/y.test.ts`, status: "failed", assertionResults: [{ fullName: "t", status: "failed" }] },
          ],
        },
        ROOT
      )
    ).toEqual([{ file: "y.test.ts", test: "t", error: "" }]);
  });
});

describe("countsOf", () => {
  it("reads vitest's totals, or zeros without a report", () => {
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
  it("marks a failure real when it fails again and flaky when it passes", () => {
    const first = [
      { file: "f.test.ts", test: "one" },
      { file: "f.test.ts", test: "two" },
    ];
    expect(classify(first, [{ file: "f.test.ts", test: "two" }]).map((failure) => failure.kind)).toEqual([
      "flaky",
      "real",
    ]);
    expect(uniqueFiles(first)).toEqual(["f.test.ts"]);
  });
});

// A fake shell: `reports` maps "<suite dir>:<all|rerun>" to the vitest report
// to write; a missing entry writes no report (a crash).
function fakeShell({ reports = {}, install = 0 } = {}) {
  const calls = [];
  const sh = (cmd, args, cwd) => {
    calls.push({ cmd, args, cwd });
    if (args[0] === "install") return { status: install, output: install ? "ERR lockfile out of date" : "" };
    const outFile = args.find((arg) => arg.startsWith("--outputFile=")).slice("--outputFile=".length);
    const files = args.slice(args.findIndex((arg) => arg.startsWith("--outputFile=")) + 1);
    const key = `${cwd.slice(ROOT.length + 1)}:${files.length ? "rerun" : "all"}`;
    if (reports[key]) writeFileSync(outFile, JSON.stringify(reports[key]));
    return { status: reports[key]?.numFailedTestSuites ? 1 : reports[key] ? 0 : 1, output: "out\nSegmentation fault" };
  };
  return { sh, calls };
}

function fakeGit() {
  const calls = [];
  const git = (args) => {
    calls.push(args.join(" "));
    if (args[0] === "rev-parse") return "0123456789abcdef";
    if (args.includes("--")) return args.at(-1).includes("b.test") ? "c0ffee1 NEWFRONT-9 change b" : "";
    return "c0ffee1 NEWFRONT-9 change b\nbeef002 NEWFRONT-8 other";
  };
  return { git, calls };
}

const green = {
  "apps/web_new:all": report([passing]),
  "packages/kit:all": report([]),
  "packages/api-client:all": report([]),
};

function run(argv, { reports = green, install } = {}) {
  const tmp = mkdtempSync(join(tmpdir(), "full-suite-test-"));
  const shell = fakeShell({ reports, install });
  const git = fakeGit();
  const written = {};
  const logs = [];
  const status = main(argv, {
    sh: shell.sh,
    git: git.git,
    root: ROOT,
    tmp,
    write: (file, text) => (written[file] = JSON.parse(text)),
    log: (line) => logs.push(line),
  });
  return { status, summary: written["s.json"], shell, git, logs };
}

describe("main", () => {
  afterEach(() => vi.restoreAllMocks());

  it("installs, runs all three suites and reports green", () => {
    const { status, summary, shell, logs } = run(["--out", "s.json"]);
    expect(status).toBe(0);
    expect(shell.calls[0]).toEqual({ cmd: "pnpm", args: ["install", "--frozen-lockfile"], cwd: ROOT });
    expect(shell.calls.slice(1).map((call) => call.cwd)).toEqual([
      `${ROOT}/apps/web_new`,
      `${ROOT}/packages/kit`,
      `${ROOT}/packages/api-client`,
    ]);
    expect(summary.head).toBe("0123456789abcdef");
    expect(summary.green).toBe(true);
    expect(summary.range).toEqual({
      since: null,
      commits: ["c0ffee1 NEWFRONT-9 change b", "beef002 NEWFRONT-8 other"],
    });
    expect(summary.suites[0]).toEqual({
      name: "web_new",
      counts: { files: 1, filesFailed: 0, tests: 1, testsFailed: 0 },
      crash: null,
      failures: [],
    });
    expect(logs.at(-1)).toBe("full-suite: 012345678 green");
  });

  it("re-runs failing files once and splits real from flaky failures", () => {
    const reports = {
      ...green,
      "apps/web_new:all": report([passing, failing, broken]),
      "apps/web_new:rerun": report([failing, { ...broken, status: "passed", message: "" }]),
    };
    const { status, summary, shell, git } = run(["--out", "s.json", "--since", "abc123", "--no-install"], { reports });
    expect(status).toBe(1);
    expect(shell.calls[0].args.slice(0, 3)).toEqual(["exec", "vitest", "run"]);
    expect(shell.calls[1].args.slice(-2)).toEqual([
      `${ROOT}/apps/web_new/src/b.test.ts`,
      `${ROOT}/apps/web_new/src/c.test.ts`,
    ]);
    expect(summary.green).toBe(false);
    expect(summary.range.since).toBe("abc123");
    expect(git.calls).toContain("log --format=%h %s abc123..HEAD");
    expect(summary.suites[0].failures).toEqual([
      {
        file: "apps/web_new/src/b.test.ts",
        test: "b breaks",
        error: "AssertionError: expected 1 to be 2\nat b.test.ts:3",
        kind: "real",
        commits: ["c0ffee1 NEWFRONT-9 change b"],
      },
      {
        file: "apps/web_new/src/c.test.ts",
        test: "(file failed to load)",
        error: "Cannot find module './gone'",
        kind: "flaky",
        commits: [],
      },
    ]);
  });

  it("stays green when every failure was flaky", () => {
    const reports = { ...green, "packages/kit:all": report([failing]), "packages/kit:rerun": report([]) };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { reports });
    expect(status).toBe(0);
    expect(summary.suites[1].failures.map((failure) => failure.kind)).toEqual(["flaky"]);
  });

  it("reports a suite that wrote no report as a crash", () => {
    const reports = { "apps/web_new:all": report([passing]), "packages/kit:all": report([]) };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { reports });
    expect(status).toBe(1);
    expect(summary.suites[2]).toEqual({
      name: "api-client",
      counts: { files: 0, filesFailed: 0, tests: 0, testsFailed: 0 },
      crash: "out\nSegmentation fault",
      failures: [],
    });
  });

  it("stops when the install fails", () => {
    expect(() => run([], { install: 1 })).toThrow("pnpm install failed:\nERR lockfile out of date");
  });

  it("removes its report folder when done", () => {
    const tmp = mkdtempSync(join(tmpdir(), "full-suite-test-"));
    main(["--out", "s.json", "--no-install"], {
      sh: fakeShell({ reports: green }).sh,
      git: fakeGit().git,
      root: ROOT,
      tmp,
      write: () => {},
      log: () => {},
    });
    expect(existsSync(tmp)).toBe(false);
  });

  it("prints the summary to stdout without --out", () => {
    const writes = [];
    vi.spyOn(process.stdout, "write").mockImplementation((text) => (writes.push(text), true));
    const { status } = run(["--no-install"]);
    expect(status).toBe(0);
    expect(JSON.parse(writes.join("")).green).toBe(true);
  });

  it("handles an empty merge range", () => {
    const tmp = mkdtempSync(join(tmpdir(), "full-suite-test-"));
    const written = {};
    main(["--out", "s.json", "--no-install"], {
      sh: fakeShell({ reports: green }).sh,
      git: (args) => (args[0] === "rev-parse" ? "fedcba987654" : ""),
      root: ROOT,
      tmp,
      write: (file, text) => (written[file] = JSON.parse(text)),
      log: () => {},
    });
    expect(written["s.json"].range.commits).toEqual([]);
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
