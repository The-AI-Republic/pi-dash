// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the scheduled full-suite runner: how vitest reports become
// failures and crashes, how re-runs split real from flaky, and what the
// summary and exit code say.
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mkdtempSync as mkProgressDir, readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
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
} from "./full-suite.mjs";
import ProgressReporter, {
  PROGRESS_ENV as PROGRESS_PATH_ENV,
  collectTests,
  fullNameOf,
  line as progressLine,
} from "./full-suite-progress.mjs";

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

describe("workerCount", () => {
  it("runs one worker per CPU on a quiet host", () => {
    expect(workerCount({ ...SCHEDULER_HOST, externalLoad: 2 })).toBe(4);
    expect(workerCount({ ...SCHEDULER_HOST, externalLoad: 0 })).toBe(4);
    expect(workerCount({ cpus: 8, memoryGiB: 30, externalLoad: 7.9 })).toBe(8);
  });

  it("oversubscribes up to the cap on a contended host", () => {
    expect(workerCount({ ...SCHEDULER_HOST, externalLoad: 16 })).toBe(16);
    // At the threshold the host is already saturated: grab share.
    expect(workerCount({ ...SCHEDULER_HOST, externalLoad: 4 })).toBe(16);
    expect(workerCount({ cpus: 2, memoryGiB: 24, externalLoad: 9 })).toBe(16);
  });

  it("lets memory shrink the pool but never grows it", () => {
    expect(workerCount({ cpus: 8, memoryGiB: 8, externalLoad: 1 })).toBe(5);
    expect(workerCount({ cpus: 8, memoryGiB: 8, externalLoad: 20 })).toBe(5);
    expect(workerCount({ cpus: 24, memoryGiB: 62, externalLoad: 5 })).toBe(16);
    expect(workerCount({ cpus: 24, memoryGiB: 62, externalLoad: 40 })).toBe(16);
  });

  it("never drops below one worker", () => {
    expect(workerCount({ cpus: 4, memoryGiB: 0.5, externalLoad: 0 })).toBe(1);
    expect(workerCount({ cpus: 4, memoryGiB: 0.5, externalLoad: 9 })).toBe(1);
  });
});

describe("workerArgs", () => {
  it("passes no flag when the pool matches vitest's default", () => {
    expect(workerArgs({ ...SCHEDULER_HOST, externalLoad: 2 })).toEqual([]);
  });

  it("passes --maxWorkers when the pool differs from the default", () => {
    expect(workerArgs({ ...SCHEDULER_HOST, externalLoad: 16 })).toEqual([WEB_WORKERS]);
    expect(workerArgs({ cpus: 8, memoryGiB: 8, externalLoad: 1 })).toEqual(["--maxWorkers=5"]);
    expect(workerArgs({ cpus: 24, memoryGiB: 62, externalLoad: 5 })).toEqual([WEB_WORKERS]);
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

describe("filesOf", () => {
  it("keeps one file per line and drops blanks, output lines and list progress markers", () => {
    const output = ["src/a.test.ts", "  ", "· src/b.test.ts", "RUN  v4 /repo", "src/c.test.ts ", ""].join("\n");
    expect(filesOf(output)).toEqual(["src/a.test.ts", "src/c.test.ts"]);
    expect(filesOf("")).toEqual([]);
  });
});

describe("progressOf", () => {
  it("keeps the last end line per file and drops start, blank and half-written lines", () => {
    const text = [
      JSON.stringify({ event: "start", file: "/a.test.ts" }),
      JSON.stringify({ event: "end", file: "/a.test.ts", state: "failed" }),
      JSON.stringify({ event: "end", file: "/a.test.ts", state: "passed" }),
      "",
      '{"event":"end","file":"/b.test.ts"',
    ].join("\n");
    const finished = progressOf(text);
    expect([...finished.keys()]).toEqual(["/a.test.ts"]);
    expect(finished.get("/a.test.ts").state).toBe("passed");
    expect(progressOf("").size).toBe(0);
    // A line that parses but is not an end event is not evidence a file ran.
    expect(progressOf('{"event":"start","file":"/c.test.ts"}\n{"what":1}\n').size).toBe(0);
  });
});

describe("reportFromFiles", () => {
  it("counts tests and files from per-file results and is only green when something ran", () => {
    expect(reportFromFiles([passing, failing])).toMatchObject({
      success: false,
      numTotalTests: 3,
      numFailedTests: 1,
    });
    expect(reportFromFiles([passing])).toMatchObject({ success: true, numTotalTests: 1, numFailedTests: 0 });
    // A file that failed to load carries no assertions but still fails.
    const loaded = reportFromFiles([broken]);
    expect(loaded.success).toBe(false);
    expect(countsOf(loaded)).toEqual({ files: 1, filesFailed: 1, tests: 0, testsFailed: 0 });
    // Stop before the first file finished: not a green report.
    expect(reportFromFiles([])).toMatchObject({ success: false, numTotalTests: 0 });
  });
});

describe("nextBatchSize and nextBatchTimeout", () => {
  it("halves a slow batch to the floor and doubles a fast one to the ceiling", () => {
    expect(nextBatchSize(120, 5 * 60_000)).toBe(60);
    expect(nextBatchSize(40, 5 * 60_000)).toBe(20);
    expect(nextBatchSize(20, 5 * 60_000)).toBe(20);
    expect(nextBatchSize(120, 10_000)).toBe(240);
    expect(nextBatchSize(300, 10_000)).toBe(400);
    expect(nextBatchSize(120, 2 * 60_000)).toBe(120);
  });

  it("gives the first batch a fixed allowance and later ones three times the last, within bounds", () => {
    expect(nextBatchTimeout(null)).toBe(12 * 60_000);
    expect(nextBatchTimeout(2 * 60_000)).toBe(6 * 60_000);
    expect(nextBatchTimeout(10_000)).toBe(3 * 60_000);
    expect(nextBatchTimeout(30 * 60_000)).toBe(20 * 60_000);
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
    // A file that hung on the re-run disproves nothing either: a hang is not
    // evidence the test passes, so the failure stays real.
    expect(classify(first, [{ file: "f.test.ts", test: "(file did not finish)" }]).map((f) => f.kind)).toEqual([
      "real",
      "real",
    ]);
  });

  it("keeps a first-run hang real when the re-run fails a real test", () => {
    expect(
      classify(
        [{ file: "f.test.ts", test: "(file did not finish)" }],
        [{ file: "f.test.ts", test: "f breaks on the re-run" }]
      )[0].kind
    ).toBe("real");
  });
});

// A fake shell for the batched runner. `files` is what `vitest list` prints;
// `plan(call)` decides what each vitest run does, given the files it was asked
// to run: { report?, progress?, status?, timedOut?, output? }. A run with no
// report writes none (the file is created only for a finished run), and its
// progress lines are appended unless the plan gives a crash.
function fakeShell({ files = ["src/a.test.ts"], install = { status: 0 }, plan = greenByDefault } = {}) {
  const calls = [];
  const sh = (cmd, args, cwd, opts) => {
    calls.push({ cmd, args, cwd, opts, env: opts?.env ?? {} });
    if (args[0] === "install") return { status: 0, output: "", timedOut: false, ...install };
    if (args.includes("list")) return { status: 0, output: files.map((file) => `${file}\n`).join(""), timedOut: false };
    const flag = args.findIndex((arg) => arg.startsWith("--outputFile="));
    const outFile = args[flag].slice("--outputFile=".length);
    const progressFile = opts.env.FULL_SUITE_PROGRESS;
    // File arguments come after every option; each is an absolute path.
    // The files this run was asked to run, as repo-relative paths.
    const runFiles = args.filter((arg) => arg.startsWith(`${ROOT}/`)).map((arg) => arg.slice(ROOT.length + 1));
    // `outFile` names the run, e.g. web_new-all-0.json or web_new-rerun-0.json.
    const label = /\/([a-z_-]+)-(all|rerun)-\d+\.json$/.exec(outFile)?.[2] ?? "all";
    const run = plan({ files: runFiles, cwd, outFile, progressFile, label }) ?? {};
    if (run.report) writeFileSync(outFile, JSON.stringify(run.report));
    if (run.progress && progressFile)
      writeFileSync(progressFile, run.progress.map((event) => `${JSON.stringify(event)}\n`).join(""));
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

// Report builders for the plan. `fileResult` makes one file entry in a vitest
// JSON report; `progressEnd` makes the matching progress line.
function fileResult(file, { failed = [], state, passed = 1 } = {}) {
  const path = `${ROOT}/${file}`;
  const passes = Array.from({ length: passed }, (_, i) => ({
    fullName: `${file} ok ${i + 1}`,
    status: "passed",
    failureMessages: [],
  }));
  const assertions = [
    ...passes,
    ...failed.map((name) => ({
      fullName: name,
      status: "failed",
      failureMessages: ["AssertionError: expected 1 to be 2", "at x.test.ts:3"],
    })),
  ];
  return {
    name: path,
    status: state ?? (failed.length > 0 ? "failed" : "passed"),
    message: "",
    startTime: 0,
    endTime: 0,
    assertionResults: assertions,
  };
}

function progressEnd(file, { failure = null, state = "passed" } = {}) {
  return {
    event: "end",
    file: `${ROOT}/${file}`,
    state: failure ? "failed" : state,
    tests: failure ? 1 : 1,
    passed: failure ? 0 : 1,
    skipped: 0,
    failures: failure ? [{ name: failure, message: "AssertionError: nope" }] : [],
    message: "",
  };
}

// Every run passes the files it was given, unless a test plans otherwise.
function greenByDefault({ files }) {
  return { report: report(files.map((file) => fileResult(file))) };
}

// Plans that answer every run with a full report of the files it was given.
function greenPlan() {
  return ({ files }) => ({ report: report(files.map((file) => fileResult(file))) });
}

function run(argv, { files, install, git = fakeGit(), plan, loadavg = () => [2, 2, 2] } = {}) {
  const tmp = mkdtempSync(join(tmpdir(), "full-suite-test-"));
  const shell = fakeShell({ files, install, plan });
  const written = {};
  // Every summary written under this name, in order: the intermediate ones a
  // stopped run leaves behind plus the final one.
  const history = [];
  const logs = [];
  const status = main(argv, {
    sh: shell.sh,
    git: git.git,
    root: ROOT,
    tmp,
    host: SCHEDULER_HOST,
    loadavg,
    write: (file, text) => {
      written[file] = JSON.parse(text);
      history.push(written[file]);
    },
    log: (line) => logs.push(line),
  });
  return { status, summary: written["s.json"], history, shell, git, logs, tmp };
}

// The vitest calls only (the install and file-listing calls are separate).
function vitestCalls(shell) {
  return shell.calls.filter((call) => call.args[0] === "exec" && !call.args.includes("list"));
}

describe("main", () => {
  afterEach(() => vi.restoreAllMocks());

  it("installs, lists each suite's files and reports green", () => {
    const { status, summary, shell, logs } = run(["--out", "s.json"]);
    expect(status).toBe(0);
    expect(shell.calls[0]).toMatchObject({ cmd: "pnpm", args: ["install", "--frozen-lockfile"], cwd: ROOT });
    // Every suite is listed, then run in one batch (it has one file).
    const listing = shell.calls.filter((call) => call.args.includes("list"));
    expect(listing.map((call) => call.cwd)).toEqual([
      `${ROOT}/apps/web_new`,
      `${ROOT}/packages/kit`,
      `${ROOT}/packages/api-client`,
    ]);
    expect(vitestCalls(shell).map((call) => call.cwd)).toEqual([
      `${ROOT}/apps/web_new`,
      `${ROOT}/packages/kit`,
      `${ROOT}/packages/api-client`,
    ]);
    expect(summary.head).toBe("0123456789abcdef");
    expect(summary.green).toBe(true);
    expect(summary.range).toEqual({
      since: null,
      requestedSince: null,
      commits: ["c0ffee1 NEWFRONT-9 change b", "beef002 NEWFRONT-8 other"],
    });
    expect(summary.suites[0]).toMatchObject({
      name: "web_new",
      counts: { files: 1, filesFailed: 0, tests: 1, testsFailed: 0 },
      crash: null,
      rerunCrash: null,
      failures: [],
    });
    expect(logs.at(-1)).toBe("full-suite: 012345678 green in 0 s");
  });

  it("passes the JSON, default and progress reporters and the progress path to every batch", () => {
    const { shell } = run(["--out", "s.json"]);
    const [webNew] = vitestCalls(shell);
    expect(webNew.args).toEqual(expect.arrayContaining(["--reporter=default", "--reporter=json"]));
    expect(webNew.args.some((arg) => arg.startsWith("--reporter=") && arg.endsWith("full-suite-progress.mjs"))).toBe(
      true
    );
    expect(webNew.env.FULL_SUITE_PROGRESS.endsWith(".jsonl")).toBe(true);
  });

  it("oversubscribes only web_new on a contended host; kit and api-client keep vitest's default", () => {
    const { shell } = run(["--out", "s.json"], { loadavg: () => [20, 18, 16] });
    const [webNew] = vitestCalls(shell);
    expect(webNew.args).toContain(WEB_WORKERS);
    expect(vitestCalls(shell)[1].args).not.toContain(WEB_WORKERS);
    expect(vitestCalls(shell)[2].args).not.toContain(WEB_WORKERS);
  });

  it("leaves every pool at vitest's default on a quiet host", () => {
    const { shell } = run(["--out", "s.json"], { loadavg: () => [2, 2, 2] });
    for (const call of vitestCalls(shell)) {
      expect(call.args.some((arg) => arg.startsWith("--maxWorkers="))).toBe(false);
    }
  });

  it("nets its own pool out of the sustained load before sizing the next batch", () => {
    const files = Array.from({ length: 250 }, (_, i) => `src/f${i}.test.ts`);
    const { shell } = run(["--out", "s.json"], { files, plan: greenPlan(), loadavg: () => [18, 18, 18] });
    const webNewRuns = vitestCalls(shell).filter((call) => call.cwd === `${ROOT}/apps/web_new`);
    expect(webNewRuns.length).toBeGreaterThan(1);
    // The first batch sees the raw average and oversubscribes; the next batch
    // subtracts the 16 workers it just ran, finds only a quiet host underneath,
    // and drops back to one worker per CPU instead of ratcheting up.
    expect(webNewRuns[0].args).toContain(WEB_WORKERS);
    expect(webNewRuns[1].args.some((arg) => arg.startsWith("--maxWorkers="))).toBe(false);
  });

  it("splits a suite larger than one batch into several runs", () => {
    const files = Array.from({ length: 250 }, (_, i) => `src/f${i}.test.ts`);
    const { status, summary, shell } = run(["--out", "s.json"], { files, plan: greenPlan() });
    expect(status).toBe(0);
    // web_new starts at 120 files per batch, and a fast batch grows the next.
    const webNewRuns = vitestCalls(shell).filter((call) => call.cwd === `${ROOT}/apps/web_new`);
    expect(webNewRuns.length).toBeGreaterThan(1);
    const sizes = webNewRuns.map((call) => call.args.filter((arg) => arg.startsWith(ROOT)).length);
    expect(sizes[0]).toBe(120);
    expect(sizes.reduce((a, b) => a + b, 0)).toBe(250);
    expect(summary.suites[0].counts).toMatchObject({ files: 250, tests: 250, filesFailed: 0 });
  });

  it("re-runs failing files once, splits real from flaky and looks up commits once per file", () => {
    const plan = ({ files, label }) =>
      label === "rerun"
        ? {
            report: report(
              files.map((file) =>
                fileResult(file, {
                  failed: file.endsWith("b.test.ts") ? ["b breaks", "b also"] : [],
                  state: file.endsWith("c.test.ts") ? undefined : undefined,
                })
              )
            ),
          }
        : {
            report: report([
              fileResult(files[0]),
              fileResult(files[1], { failed: ["b breaks", "b also"], passed: 0 }),
              fileResult(files[2], { state: "failed", passed: 0 }),
            ]),
          };
    const { status, summary, shell, git } = run(["--out", "s.json", "--since", "abc123", "--no-install"], {
      files: ["src/a.test.ts", "src/b.test.ts", "src/c.test.ts"],
      plan,
    });
    expect(status).toBe(1);
    // The re-run names exactly the failing files.
    const rerun = vitestCalls(shell)[1];
    expect(rerun.args.filter((arg) => arg.startsWith(ROOT))).toEqual([
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
    const plan = ({ files, label }) =>
      label === "rerun"
        ? { report: report(files.map((file) => fileResult(file))) }
        : { report: report(files.map((file) => fileResult(file, { failed: ["g breaks"] }))) };
    const { status, summary } = run(["--out", "s.json", "--no-install"], {
      files: ["src/g.test.ts"],
      plan,
    });
    expect(status).toBe(0);
    expect(summary.suites[0].failures.map((failure) => failure.kind)).toEqual(["flaky"]);
  });

  it("says so when the re-run never reached its files, and keeps the failures real", () => {
    // Every failing file also stalls alone on the re-run; the re-run's own
    // deadline runs out before any of them runs.
    const plan = ({ files, label }) =>
      label === "rerun"
        ? { timedOut: true }
        : { report: report(files.map((file) => fileResult(file, { failed: ["a breaks"] }))) };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files: ["src/a.test.ts"], plan });
    expect(status).toBe(1);
    expect(summary.suites[0].rerunCrash).toBeNull();
    // The re-run itself named the file that hung, and the original failure is
    // kept as real (not "flaky") because nothing proved it passes.
    expect(summary.suites[0].failures[0]).toMatchObject({ test: "a breaks", kind: "real" });
  });

  it("says so when the run's budget ran out before the re-run could start", () => {
    // The first suite runs past the whole shared budget (42 min): the re-run
    // still names its failures but never starts, and nothing is disproved.
    const real = Date.now.bind(Date);
    let spent = false;
    const clock = vi.spyOn(Date, "now").mockImplementation(() => real() + (spent ? 50 * 60_000 : 0));
    try {
      const plan = ({ files, label }) => {
        if (label === "all") spent = true;
        return { report: report(files.map((file) => fileResult(file, { failed: ["a breaks"] }))) };
      };
      const { status, summary } = run(["--out", "s.json", "--no-install"], { files: ["src/a.test.ts"], plan });
      expect(status).toBe(1);
      expect(summary.suites[0].rerunCrash).toBe("re-run did not run 1 file(s)");
      expect(summary.suites[0].failures.map((failure) => failure.kind)).toEqual(["real"]);
    } finally {
      clock.mockRestore();
    }
  });

  it("keeps failures real when the re-run crashes", () => {
    const plan = ({ files, label }) =>
      label === "rerun"
        ? { status: 1, output: "Segmentation fault" }
        : { report: report(files.map((file) => fileResult(file, { failed: ["g breaks"] }))) };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files: ["src/g.test.ts"], plan });
    expect(status).toBe(1);
    expect(summary.suites[0].rerunCrash).toBe("Segmentation fault");
    expect(summary.suites[0].failures[0].kind).toBe("real");
  });

  it("leaves a red partial summary on disk after each suite, and a complete one at the end", () => {
    const { status, history, summary } = run(["--out", "s.json", "--no-install"]);
    expect(status).toBe(0);
    // A red summary lands before the first suite runs, then after each one;
    // the last write is the complete one. A run stopped at any point still
    // leaves the head, the merge range and every suite finished so far.
    expect(history.map((entry) => [entry.complete, entry.green, entry.suites.length])).toEqual([
      [false, false, 0],
      [false, false, 1],
      [false, false, 2],
      [true, true, 3],
    ]);
    expect(summary.complete).toBe(true);
  });

  it("reports a suite whose file listing failed as a crash", () => {
    const shell = fakeShell();
    const sh = (cmd, args, cwd, opts) =>
      args.includes("list") ? { status: 1, output: "boom" } : shell.sh(cmd, args, cwd, opts);
    const written = {};
    const status = main(["--out", "s.json", "--no-install"], {
      sh,
      git: fakeGit().git,
      root: ROOT,
      tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
      host: SCHEDULER_HOST,
      write: (file, text) => (written[file] = JSON.parse(text)),
      log: () => {},
    });
    expect(status).toBe(1);
    expect(written["s.json"].suites[0].crash).toBe("vitest list --filesOnly failed");
    expect(written["s.json"].suites[0].counts).toEqual({ files: 0, filesFailed: 0, tests: 0, testsFailed: 0 });
  });

  it("reports a suite that lists no files as a crash", () => {
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files: [], plan: greenPlan() });
    expect(status).toBe(1);
    expect(summary.suites[0].crash).toBe("vitest listed no test files");
  });

  it("reports a failed run with no failing test as a crash", () => {
    const plan = ({ files }) => ({
      report: report(
        files.map((file) => fileResult(file)),
        { success: false }
      ),
      status: 1,
      output: "Unhandled Rejection\nboom",
    });
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files: ["src/a.test.ts"], plan });
    expect(status).toBe(1);
    expect(summary.suites[0].crash).toBe("Unhandled Rejection\nboom");
  });

  it("says which files did not finish, naming them, when a batch times out", () => {
    const files = ["src/a.test.ts", "src/b.test.ts"];
    // The batch stalls; each file retried alone stalls too.
    const plan = () => ({ timedOut: true, output: "still running" });
    const { status, summary, shell } = run(["--out", "s.json", "--no-install"], { files, plan });
    expect(status).toBe(1);
    // The whole batch stalled, then each file was retried alone and also stalled.
    expect(summary.suites[0].crash).toBe(null);
    expect(summary.suites[0].counts).toMatchObject({ files: 2, filesFailed: 2 });
    expect(summary.suites[0].failures.map((failure) => failure.file)).toEqual([
      "apps/web_new/src/a.test.ts",
      "apps/web_new/src/b.test.ts",
    ]);
    expect(summary.suites[0].failures.every((failure) => failure.test === "(file did not finish)")).toBe(true);
    // The initial batch, then one retry per file; the re-run of the two
    // failures hangs the same way and is bisected again.
    expect(vitestCalls(shell).length).toBeGreaterThanOrEqual(3);
  });

  it("reports a timed-out batch as a crash even when every file it reported passed", () => {
    const files = ["src/a.test.ts", "src/b.test.ts"];
    const plan = ({ files: run_, label }) =>
      label === "rerun"
        ? { report: report(run_.map((file) => fileResult(file))) }
        : {
            progress: run_.map((file) => progressEnd(file)),
            timedOut: true,
            output: "worker still running after all modules ended",
          };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files, plan });
    expect(status).toBe(1);
    expect(summary.suites[0].counts).toMatchObject({ files: 2, filesFailed: 0 });
    expect(summary.suites[0].crash).toContain("worker still running");
    expect(summary.suites[0].failures).toEqual([]);
  });

  it("reports a non-zero run with no JSON report as a crash when every file did finish", () => {
    const files = ["src/a.test.ts", "src/b.test.ts"];
    const plan = ({ files: run_, label }) =>
      label === "rerun"
        ? { report: report(run_.map((file) => fileResult(file))) }
        : { progress: run_.map((file) => progressEnd(file)), status: 2, output: "worker exited 2" };
    const { status, summary } = run(["--out", "s.json", "--no-install"], { files, plan });
    expect(status).toBe(1);
    expect(summary.suites[0].counts).toMatchObject({ files: 2, filesFailed: 0 });
    expect(summary.suites[0].crash).toBe("worker exited 2");
  });

  it("classifies failures recovered from progress lines by test name", () => {
    const plan = ({ files, label }) =>
      label === "rerun"
        ? { report: report(files.map((file) => fileResult(file))) }
        : {
            progress: files.map((file) =>
              progressEnd(file, { failure: file.endsWith("b.test.ts") ? "b > breaks" : null })
            ),
            timedOut: true,
            output: "still running",
          };
    const { summary } = run(["--out", "s.json", "--no-install"], {
      files: ["src/a.test.ts", "src/b.test.ts"],
      plan,
    });
    const [webNew] = summary.suites;
    // a finished, b failed in the stalled batch and passed on its own re-run.
    expect(webNew.counts).toMatchObject({ files: 2, filesFailed: 1, tests: 2, testsFailed: 1 });
    expect(webNew.failures).toEqual([
      expect.objectContaining({ file: "apps/web_new/src/b.test.ts", test: "b > breaks", kind: "flaky" }),
    ]);
  });

  it("keeps the files a stalled batch finished, and bisects only the rest", () => {
    const files = ["src/a.test.ts", "src/b.test.ts", "src/c.test.ts"];
    const plan = ({ files: run_ }) =>
      run_.length > 1
        ? { progress: [progressEnd(run_[0])], timedOut: true, output: "still running" }
        : run_[0].endsWith("b.test.ts")
          ? { timedOut: true, output: "stuck" }
          : { report: report([fileResult(run_[0])]) };
    const { summary } = run(["--out", "s.json", "--no-install"], { files, plan });
    const [webNew] = summary.suites;
    expect(webNew.counts).toMatchObject({ files: 3, filesFailed: 1 });
    expect(webNew.failures).toHaveLength(1);
    expect(webNew.failures[0]).toMatchObject({ file: "apps/web_new/src/b.test.ts", test: "(file did not finish)" });
  });

  it("stops spending the budget and names the files it never reached", () => {
    // A deadline already in the past: the batch loop must not start a run.
    const { sh } = fakeShell({ files: ["src/a.test.ts"] });
    const run_ = runSuiteFiles(
      { name: "web_new", dir: "apps/web_new", batchFiles: 1 },
      {
        sh,
        tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
        root: ROOT,
        host: SCHEDULER_HOST,
        log: () => {},
        deadline: Date.now() - 1,
      }
    );
    expect(run_.results).toEqual([]);
    expect(run_.unfinished).toEqual(["src/a.test.ts"]);
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

  it("prints the summary to stdout without --out and removes its temp folder", () => {
    const writes = [];
    vi.spyOn(process.stdout, "write").mockImplementation((text) => (writes.push(text), true));
    const { status, tmp } = run(["--no-install"], { plan: greenPlan() });
    expect(status).toBe(0);
    expect(JSON.parse(writes.join("")).green).toBe(true);
    expect(existsSync(tmp)).toBe(false);
  });

  it("handles an empty merge range", () => {
    const git = { git: (args) => (args[0] === "rev-parse" ? "fedcba987654" : "") };
    expect(run(["--out", "s.json", "--no-install"], { git }).summary.range.commits).toEqual([]);
  });
});

// Task-tree builders for the reporter tests. `suiteNode` and `testNode` make
// the shape vitest hands a reporter: suites nest, test cases carry a result
// and any errors, and `moduleOf` wraps them in the module record.
function suiteNode(name, tasks = [], result, parent) {
  return { type: "suite", name, tasks, result, suite: parent };
}

function testNode(name, state = "pass", errors = [], parent) {
  return { type: "test", name, result: { state, errors }, suite: parent };
}

function moduleOf(file, task) {
  return { moduleId: file, task, result: task.result, errors: [] };
}

function reporterAt() {
  const dir = mkProgressDir(join(tmpdir(), "progress-test-"));
  const path = join(dir, "events.jsonl");
  return { path, reporter: new ProgressReporter({ path }), read: () => readFileSync(path, "utf8") };
}

describe("collectTests", () => {
  it("walks nested suites and records failed test names and messages", () => {
    const inner = suiteNode("describe inner", [
      testNode("passes", "pass", [], undefined),
      testNode("fails", "fail", [{ stack: "AssertionError: nope\n  at a.test.ts:1" }]),
    ]);
    inner.tasks.forEach((task) => (task.suite = inner));
    const outer = suiteNode("describe outer", [inner, testNode("skipped", "skip")]);
    inner.suite = outer;
    outer.tasks[1].suite = outer;
    expect(collectTests(outer)).toEqual({
      total: 3,
      passed: 1,
      skipped: 1,
      failed: [{ name: "describe outer > describe inner > fails", message: "AssertionError: nope\n  at a.test.ts:1" }],
    });
  });

  it("copes with a task tree that has no children", () => {
    expect(collectTests({})).toEqual({ total: 0, passed: 0, skipped: 0, failed: [] });
    expect(collectTests(undefined)).toEqual({ total: 0, passed: 0, skipped: 0, failed: [] });
  });
});

describe("fullNameOf", () => {
  it("joins the describe titles from the outside in with the test name", () => {
    const root = suiteNode("file", []);
    const mid = suiteNode("middle", [], undefined, root);
    const testCase = testNode("leaf", "pass", [], mid);
    expect(fullNameOf(testCase)).toBe("file > middle > leaf");
    expect(fullNameOf(testNode("alone", "pass", [], { name: "" }))).toBe("alone");
  });
});

describe("ProgressReporter", () => {
  it("writes a start line and an end line per module, at module end", () => {
    const { reporter, read, path } = reporterAt();
    const task = suiteNode("file", [testNode("ok")]);
    reporter.onTestModuleStart(moduleOf("/repo/src/a.test.ts", task));
    reporter.onTestModuleEnd(moduleOf("/repo/src/a.test.ts", task));
    const events = read()
      .trim()
      .split("\n")
      .map((raw) => JSON.parse(raw));
    expect(events.map((event) => event.event)).toEqual(["start", "end"]);
    expect(events[0]).toEqual({ event: "start", file: "/repo/src/a.test.ts" });
    expect(events[1]).toMatchObject({
      event: "end",
      file: "/repo/src/a.test.ts",
      state: "passed",
      tests: 1,
      passed: 1,
      skipped: 0,
      failures: [],
      message: "",
    });
    // Each line is complete on its own, so a reader can stop mid-file.
    expect(read().endsWith("\n")).toBe(true);
    expect(path.endsWith(".jsonl")).toBe(true);
  });

  it("marks a module failed and keeps the failed test names", () => {
    const { reporter, read } = reporterAt();
    const task = suiteNode("file", [testNode("breaks", "fail", [{ message: "boom" }])], { state: "fail" }, undefined);
    task.tasks.forEach((child) => (child.suite = task));
    reporter.onTestModuleEnd(moduleOf("/repo/src/b.test.ts", task));
    const [event] = read()
      .trim()
      .split("\n")
      .map((raw) => JSON.parse(raw));
    expect(event).toMatchObject({
      state: "failed",
      tests: 1,
      failures: [{ name: "file > breaks", message: "boom" }],
    });
  });

  it("falls back to the module's first error as the file message, and to its task name without moduleId", () => {
    const { reporter, read } = reporterAt();
    const task = suiteNode("src/c.test.ts", []);
    task.result = { state: "fail" };
    reporter.onTestModuleStart({ task });
    reporter.onTestModuleEnd({ task, result: { state: "fail" }, errors: [{ message: "Cannot find module './gone'" }] });
    const events = read()
      .trim()
      .split("\n")
      .map((raw) => JSON.parse(raw));
    expect(events[1]).toMatchObject({ event: "end", file: "src/c.test.ts" });
    expect(events[1].message.split("\n")[0]).toBe("Cannot find module './gone'");
    expect(events[0]).toEqual({ event: "start", file: "src/c.test.ts" });
  });

  it("writes nothing when it has no path", () => {
    const reporter = new ProgressReporter({ path: "" });
    // No throw, no file: the runner always passes a path, but a reporter run
    // by hand must not crash the suite it is attached to.
    expect(() => reporter.onTestModuleStart(moduleOf("/repo/x.test.ts", suiteNode("x", [])))).not.toThrow();
    expect(() => reporter.onTestModuleEnd(moduleOf("/repo/x.test.ts", suiteNode("x", [])))).not.toThrow();
  });

  it("describes a test with no result and errors with no stack or message", () => {
    expect(collectTests({ tasks: [{ type: "test", name: "bare" }] })).toEqual({
      total: 1,
      passed: 0,
      skipped: 1,
      failed: [],
    });
    // A failed test with a result but no errors list still names itself.
    expect(collectTests({ tasks: [{ type: "test", name: "quiet", result: { state: "fail" } }] })).toEqual({
      total: 1,
      passed: 0,
      skipped: 0,
      failed: [{ name: "quiet", message: "" }],
    });
    const { reporter, read } = reporterAt();
    reporter.onTestModuleEnd({
      task: { tasks: [{ type: "test", name: "odd", result: { state: "fail", errors: [{}] } }] },
      result: { state: "fail" },
      errors: [{}],
    });
    const [event] = read()
      .trim()
      .split("\n")
      .map((raw) => JSON.parse(raw));
    expect(event.failures).toEqual([{ name: "odd", message: "" }]);
    expect(event.message).toBe("");
  });

  it("leaves the path empty when neither an option nor the environment names one", () => {
    const previous = process.env[PROGRESS_PATH_ENV];
    delete process.env[PROGRESS_PATH_ENV];
    try {
      const reporter = new ProgressReporter();
      expect(reporter.path).toBe("");
    } finally {
      if (previous !== undefined) process.env[PROGRESS_PATH_ENV] = previous;
    }
  });

  it("falls back to the module's message when it has no stack, and to an unnamed task", () => {
    const { reporter, read } = reporterAt();
    reporter.onTestModuleEnd({
      task: { tasks: [{ type: "test", name: "plain", result: { state: "fail", errors: [{ message: "no stack" }] } }] },
      result: { state: "fail" },
      errors: [{ message: "module-level" }],
    });
    reporter.onTestModuleStart({});
    const events = read()
      .trim()
      .split("\n")
      .map((raw) => JSON.parse(raw));
    expect(events[0]).toMatchObject({ event: "end", file: "", state: "failed", message: "module-level" });
    expect(events[0].failures).toEqual([{ name: "plain", message: "no stack" }]);
    expect(events[1]).toEqual({ event: "start", file: "" });
  });

  it("writes an empty message for a failed module that carries no error", () => {
    const { reporter, read } = reporterAt();
    reporter.onTestModuleEnd({ task: { tasks: [] }, result: { state: "fail" } });
    expect(JSON.parse(read().trim())).toMatchObject({ state: "failed", message: "", tests: 0, failures: [] });
  });

  it("reads the path from FULL_SUITE_PROGRESS when no option is passed", () => {
    const dir = mkProgressDir(join(tmpdir(), "progress-test-"));
    const path = join(dir, "env.jsonl");
    const previous = process.env[PROGRESS_PATH_ENV];
    process.env[PROGRESS_PATH_ENV] = path;
    try {
      new ProgressReporter().onTestModuleStart(moduleOf("/repo/src/d.test.ts", suiteNode("d", [])));
      expect(JSON.parse(readFileSync(path, "utf8"))).toEqual({ event: "start", file: "/repo/src/d.test.ts" });
    } finally {
      if (previous === undefined) delete process.env[PROGRESS_PATH_ENV];
      else process.env[PROGRESS_PATH_ENV] = previous;
    }
  });
});

describe("progress line", () => {
  it("serializes one event per line", () => {
    expect(progressLine({ event: "end" })).toBe('{"event":"end"}\n');
  });
});

describe("progressResult", () => {
  it("rebuilds passed, failed and skipped assertions from a progress line", () => {
    const result = progressResult("/repo/a.test.ts", {
      state: "failed",
      passed: 2,
      skipped: 1,
      failures: [{ name: "a > breaks", message: "boom" }],
    });
    expect(result.status).toBe("failed");
    expect(result.assertionResults.map((assertion) => [assertion.fullName, assertion.status])).toEqual([
      ["/repo/a.test.ts passed #1", "passed"],
      ["/repo/a.test.ts passed #2", "passed"],
      ["a > breaks", "failed"],
      ["/repo/a.test.ts skipped #1", "pending"],
    ]);
    expect(failuresOf({ testResults: [result] }, ROOT)).toEqual([
      { file: "a.test.ts", test: "a > breaks", error: "boom" },
    ]);
  });

  it("falls back to an empty failure message and to no failures array at all", () => {
    const noMessage = progressResult("/repo/e.test.ts", { state: "failed", failures: [{ name: "e > breaks" }] });
    expect(noMessage.assertionResults.at(-1).failureMessages).toEqual([""]);
    const legacy = progressResult("/repo/f.test.ts", { state: "passed", passed: 1 });
    expect(legacy.assertionResults).toEqual([
      { fullName: "/repo/f.test.ts passed #1", status: "passed", failureMessages: [] },
    ]);
  });

  it("copes with an older line that has only counts, and names the failure it cannot", () => {
    const named = progressResult("/repo/b.test.ts", { state: "failed", failures: [{ message: "no name" }] });
    expect(named.assertionResults.at(-1)).toMatchObject({
      fullName: "/repo/b.test.ts failed",
      failureMessages: ["no name"],
    });
    // A failed file with no recorded failures keeps the file-level marker.
    const bare = progressResult("/repo/c.test.ts", { state: "failed", message: "worker died" });
    expect(bare.assertionResults).toEqual([
      { fullName: "(file did not finish)", status: "failed", failureMessages: ["worker died"] },
    ]);
    // No message either: the marker is still there, with an empty excerpt.
    expect(progressResult("/repo/g.test.ts", { state: "failed" }).assertionResults).toEqual([
      { fullName: "(file did not finish)", status: "failed", failureMessages: [""] },
    ]);
    const passed = progressResult("/repo/d.test.ts", { state: "passed" });
    expect(passed).toMatchObject({ status: "passed", message: "", assertionResults: [] });
  });
});

describe("suiteRelative", () => {
  it("strips the suite's directory and leaves another suite's path alone", () => {
    const suite = { dir: "apps/web_new" };
    expect(suiteRelative(suite, "apps/web_new/src/a.test.ts")).toBe("src/a.test.ts");
    expect(suiteRelative(suite, "src/a.test.ts")).toBe("src/a.test.ts");
  });
});

describe("bisecting a stalled batch", () => {
  const suite = { name: "web_new", dir: "apps/web_new", batchFiles: 3 };

  it("names every file still stuck together once the halving limit is reached", () => {
    // Three files always stall, so no split ever isolates one: at the depth
    // limit the remaining group is reported file by file, with the output tail.
    const { sh } = fakeShell({
      plan: () => ({ timedOut: true, output: "line one\nline two", progress: [] }),
    });
    const out = runBatchIsolated(
      suite,
      ["src/a.test.ts", "src/b.test.ts"],
      {
        sh,
        tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
        root: ROOT,
        host: SCHEDULER_HOST,
        deadline: Date.now() + 60_000,
        lastElapsedMs: null,
        seq: 0,
      },
      3
    );
    expect(out.crashed).toBe(false);
    expect(out.unfinished).toEqual([]);
    expect(out.results.map((result) => suiteRelative(suite, result.name.replace(`${ROOT}/`, "")))).toEqual([
      "src/a.test.ts",
      "src/b.test.ts",
    ]);
    expect(out.results[0].message).toContain("2 file(s) did not finish together after 3 splits");
    expect(out.results[0].message).toContain("line one");
  });

  it("starts no run at all when the deadline has already passed", () => {
    const { sh, calls } = fakeShell();
    const out = runBatchIsolated(suite, ["src/a.test.ts"], {
      sh,
      tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
      root: ROOT,
      host: SCHEDULER_HOST,
      deadline: Date.now() - 1,
      lastElapsedMs: null,
      seq: 0,
    });
    expect(out).toEqual({ results: [], unfinished: ["src/a.test.ts"], runs: 0, crashed: false, output: "" });
    expect(calls).toEqual([]);
  });
});

describe("runSuiteFiles", () => {
  it("keeps the first crash's output and logs each batch", () => {
    const files = ["src/a.test.ts", "src/b.test.ts"];
    const { sh } = fakeShell({
      files,
      plan: () => ({ report: report([fileResult("src/a.test.ts")]), status: 1, output: "Unhandled Rejection\nboom" }),
    });
    const logs = [];
    const out = runSuiteFiles(
      { name: "kit", dir: "packages/kit", batchFiles: 1 },
      {
        sh,
        tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
        root: ROOT,
        host: SCHEDULER_HOST,
        log: (line) => logs.push(line),
      }
    );
    expect(out.crashed).toBe(true);
    expect(out.output).toContain("Unhandled Rejection");
    expect(logs).toEqual(["full-suite: kit batch of 1 took 0 s", "full-suite: kit batch of 1 took 0 s"]);
  });

  it("starts a suite without a batch size of its own at the shared default", () => {
    const files = Array.from({ length: 250 }, (_, i) => `src/f${i}.test.ts`);
    const { sh, calls } = fakeShell({ files, plan: greenPlan() });
    const out = runSuiteFiles(
      { name: "kit", dir: "packages/kit" },
      { sh, tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")), root: ROOT, host: SCHEDULER_HOST, log: () => {} }
    );
    const sizes = calls
      .filter((call) => call.args[0] === "exec" && !call.args.includes("list"))
      .map((call) => call.args.filter((arg) => arg.startsWith(ROOT)).length);
    expect(sizes[0]).toBe(200);
    expect(sizes.reduce((a, b) => a + b, 0)).toBe(250);
    expect(out.results).toHaveLength(250);
    expect(out.unfinished).toEqual([]);
    expect(out.error).toBeNull();
  });
});

describe("runSuite", () => {
  const suite = { name: "web_new", dir: "apps/web_new", batchFiles: 2 };

  it("names the files a spent budget never reached, and counts the ones that did run", () => {
    // The first batch passes but holds past the deadline, so the second never
    // starts: the report still holds the first two files and the crash names
    // the file that was left unrun.
    const { sh } = fakeShell({
      files: ["src/a.test.ts", "src/b.test.ts", "src/c.test.ts"],
      plan: ({ files }) => {
        const until = Date.now() + 40;
        while (Date.now() < until) {
          /* hold the batch past the deadline */
        }
        return { report: report(files.map((file) => fileResult(file))) };
      },
    });
    const out = runSuite(suite, {
      sh,
      tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
      root: ROOT,
      host: SCHEDULER_HOST,
      log: () => {},
      deadline: Date.now() + 20,
    });
    expect(out.ranFiles).toBe(2);
    expect(out.report.testResults).toHaveLength(2);
    expect(out.crash).toBe("suite budget spent: 1 file(s) not run\nsrc/c.test.ts");
  });

  it("truncates a long list of unrun files to a 30-line excerpt", () => {
    const files = Array.from({ length: 35 }, (_, i) => `src/f${i}.test.ts`);
    const { sh } = fakeShell({ files });
    const out = runSuite(suite, {
      sh,
      tmp: mkdtempSync(join(tmpdir(), "full-suite-test-")),
      root: ROOT,
      host: SCHEDULER_HOST,
      log: () => {},
      deadline: Date.now() - 1,
    });
    expect(out.crash.split("\n")[0]).toBe("suite budget spent: 35 file(s) not run");
    expect(out.crash).toContain("src/f29.test.ts");
    expect(out.crash).not.toContain("src/f30.test.ts");
    expect(out.crash.endsWith("(+5 more)")).toBe(true);
    expect(out.report).toMatchObject({ success: false, testResults: [] });
    expect(out.ranFiles).toBe(0);
  });
});

describe("run deadlines", () => {
  const now = 1_000_000;
  const minute = 60_000;

  it("stops a suite at its own allowance or at the run's shared budget, whichever ends first", () => {
    expect(suiteDeadline(now + 90 * minute, now)).toBe(now + 45 * minute);
    expect(suiteDeadline(now + 10 * minute, now)).toBe(now + 10 * minute);
    expect(suiteDeadline(now, now)).toBe(now);
  });

  it("clips a re-run's allowance to the shared budget and keeps a floor for a slow file list", () => {
    expect(rerunDeadline(1, now + 90 * minute, now)).toBe(now + 3 * minute);
    expect(rerunDeadline(100, now + 90 * minute, now)).toBe(now + 20 * minute);
    expect(rerunDeadline(100, now + 2 * minute, now)).toBe(now + 2 * minute);
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
