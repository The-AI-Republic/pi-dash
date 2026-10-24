// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the changed-tests runner: which changes are left out of the
// run, how arguments reach vitest, and how the changed-file list is gathered.
import { describe, expect, it } from "vitest";

import { changedFilesSince, isCliEntry, main, parseArgs, planRun, skippedFiles } from "./test-changed.mjs";

describe("skippedFiles", () => {
  it("leaves out inputs that reach tests without an import edge", () => {
    const files = [
      "pnpm-lock.yaml",
      "package.json",
      "apps/web_new/package.json",
      "packages/kit/package.json",
      "packages/api-client/package.json",
      "apps/web_new/vitest.config.ts",
      "apps/web_new/vite.config.ts",
      "apps/web_new/tsconfig.json",
      "packages/kit/tsconfig.build.json",
      "apps/web_new/src/test-setup.ts",
    ];
    expect(skippedFiles(files)).toEqual(files);
  });

  it("keeps ordinary source, test, kit and api-client changes for the import graph", () => {
    expect(
      skippedFiles([
        "apps/web_new/src/features/issues/create/createApi.ts",
        "apps/web_new/e2e/parity/drivers/web-new-modules-shell.test.ts",
        "packages/kit/src/components/Button.tsx",
        "packages/api-client/src/index.ts",
        ".ai_design/new_frontend_implementation/parity/inventory/drafts.md",
      ])
    ).toEqual([]);
  });
});

describe("parseArgs", () => {
  it("reads --base in both spellings, --full, and passes the rest to vitest", () => {
    expect(parseArgs(["--base", "abc", "--coverage"])).toEqual({
      base: "abc",
      full: false,
      vitestArgs: ["--coverage"],
    });
    expect(parseArgs(["--base=HEAD^1", "--full"])).toEqual({ base: "HEAD^1", full: true, vitestArgs: [] });
    expect(parseArgs([])).toEqual({ base: null, full: false, vitestArgs: [] });
  });

  it("passes option values through and refuses path filters", () => {
    expect(parseArgs(["--reporter", "json", "--coverage"]).vitestArgs).toEqual(["--reporter", "json", "--coverage"]);
    expect(() => parseArgs(["src/features/foo"])).toThrow("test:changed takes no path filters (src/features/foo)");
    expect(() => parseArgs(["--reporter=json", "src/a.ts"])).toThrow("takes no path filters (src/a.ts)");
  });

  it("refuses --base without a ref", () => {
    expect(() => parseArgs(["--base"])).toThrow("--base needs a ref");
  });
});

describe("planRun", () => {
  it("passes the changed files to vitest related as absolute paths", () => {
    expect(
      planRun({ full: false, vitestArgs: ["--coverage"], changedFiles: ["apps/web_new/src/a.ts"], root: "/repo" })
    ).toEqual({
      mode: "changed",
      reason: null,
      skipped: [],
      args: ["related", "--run", "--passWithNoTests", "--coverage", "/repo/apps/web_new/src/a.ts"],
    });
  });

  it("runs only the source changes when a config or manifest changed too", () => {
    expect(
      planRun({
        full: false,
        vitestArgs: [],
        changedFiles: ["apps/web_new/vitest.config.ts", "apps/web_new/src/x.ts", "pnpm-lock.yaml"],
        root: "/repo",
      })
    ).toEqual({
      mode: "changed",
      reason: null,
      skipped: ["apps/web_new/vitest.config.ts", "pnpm-lock.yaml"],
      args: ["related", "--run", "--passWithNoTests", "/repo/apps/web_new/src/x.ts"],
    });
  });

  it("runs the whole suite when too many files changed for one command line", () => {
    const changedFiles = Array.from({ length: 1001 }, (_, i) => `apps/web_new/src/f${i}.ts`);
    expect(planRun({ full: false, vitestArgs: ["--coverage"], changedFiles, root: "/repo" })).toEqual({
      mode: "full",
      reason: "1001 changed files",
      skipped: [],
      args: ["run", "--coverage"],
    });
  });

  it("runs the whole suite on --full", () => {
    expect(planRun({ full: true, vitestArgs: ["--coverage"], changedFiles: [], root: "" })).toEqual({
      mode: "full",
      reason: "--full",
      skipped: [],
      args: ["run", "--coverage"],
    });
  });
});

// A fake git that answers the commands the runner issues.
function fakeGit({ diff = "", untracked = "" } = {}) {
  const calls = [];
  const git = (args) => {
    calls.push(args.join(" "));
    if (args[0] === "rev-parse") return "/repo";
    if (args[0] === "merge-base") return "mb123";
    if (args[2] === "diff") return diff;
    if (args[2] === "ls-files") return untracked;
    throw new Error(`unexpected git ${args.join(" ")}`);
  };
  return { git, calls };
}

describe("changedFilesSince", () => {
  it("merges tracked and untracked changes without duplicates or blanks", () => {
    const { git, calls } = fakeGit({ diff: "a.ts\nb.ts", untracked: "b.ts\nc.ts" });
    expect(changedFilesSince("base1", git)).toEqual(["a.ts", "b.ts", "c.ts"]);
    expect(calls).toEqual([
      "rev-parse --show-toplevel",
      "-C /repo diff --name-only base1",
      "-C /repo ls-files --others --exclude-standard",
    ]);
  });

  it("returns nothing when nothing changed", () => {
    expect(changedFilesSince("base1", fakeGit().git)).toEqual([]);
  });
});

describe("main", () => {
  it("defaults the base to the merge base with origin/web-new-dev and runs affected tests", () => {
    const { git, calls } = fakeGit({ diff: "apps/web_new/src/a.ts" });
    const runs = [];
    const logs = [];
    const status = main(["--coverage"], {
      git,
      runVitest: (args) => (runs.push(args), 0),
      log: (line) => logs.push(line),
    });
    expect(status).toBe(0);
    expect(calls[0]).toBe("merge-base HEAD origin/web-new-dev");
    expect(runs).toEqual([["related", "--run", "--passWithNoTests", "--coverage", "/repo/apps/web_new/src/a.ts"]]);
    expect(logs).toEqual(["test:changed: tests affected by 1 file(s) changed since mb123"]);
  });

  it("uses an explicit base, reports the files it leaves out, and returns vitest's status", () => {
    const { git, calls } = fakeGit({ diff: "apps/web_new/vitest.config.ts" });
    const runs = [];
    const logs = [];
    const status = main(["--base", "HEAD^1"], {
      git,
      runVitest: (args) => (runs.push(args), 1),
      log: (line) => logs.push(line),
    });
    expect(status).toBe(1);
    expect(calls).not.toContain("merge-base HEAD origin/web-new-dev");
    expect(runs).toEqual([["related", "--run", "--passWithNoTests"]]);
    expect(logs).toEqual([
      "test:changed: tests affected by 1 file(s) changed since HEAD^1",
      "test:changed: no import edge, left to the scheduled full suite: apps/web_new/vitest.config.ts",
    ]);
  });

  it("reports why it runs the whole suite for a very large diff", () => {
    const diff = Array.from({ length: 1001 }, (_, i) => `apps/web_new/src/f${i}.ts`).join("\n");
    const logs = [];
    main([], { git: fakeGit({ diff }).git, runVitest: () => 0, log: (line) => logs.push(line) });
    expect(logs).toEqual(["test:changed: full suite (1001 changed files since mb123)"]);
  });

  it("skips the diff entirely on --full", () => {
    const { git, calls } = fakeGit();
    const runs = [];
    main(["--full", "--base", "x"], { git, runVitest: (args) => (runs.push(args), 0), log: () => {} });
    expect(calls).toEqual([]);
    expect(runs).toEqual([["run"]]);
  });
});

describe("main with the real git", () => {
  it("diffs this checkout against an explicit base", () => {
    const runs = [];
    main(["--base", "HEAD"], { runVitest: (args) => (runs.push(args), 0), log: () => {} });
    expect(runs).toHaveLength(1);
    expect(runs[0].slice(0, 2)).toEqual(["related", "--run"]);
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
