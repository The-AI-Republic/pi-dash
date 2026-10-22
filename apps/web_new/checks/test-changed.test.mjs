// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the changed-tests runner: which changes force the whole suite,
// how arguments reach vitest, and how the changed-file list is gathered.
import { describe, expect, it } from "vitest";

import { changedFilesSince, fullSuiteReasons, isCliEntry, main, parseArgs, planRun } from "./test-changed.mjs";

describe("fullSuiteReasons", () => {
  it("flags inputs that reach every test without an import edge", () => {
    const files = [
      "pnpm-lock.yaml",
      "package.json",
      "apps/web_new/package.json",
      "packages/kit/package.json",
      "packages/api-client/package.json",
      "apps/web_new/vitest.config.ts",
      "apps/web_new/vite.config.ts",
      "apps/web_new/tsconfig.json",
      "apps/web_new/src/test-setup.ts",
    ];
    expect(fullSuiteReasons(files)).toEqual(files);
  });

  it("leaves ordinary source, kit and api-client changes to the import graph", () => {
    expect(
      fullSuiteReasons([
        "apps/web_new/src/features/issues/create/createApi.ts",
        "packages/kit/src/components/Button.tsx",
        "packages/api-client/src/index.ts",
        "apps/web/package.json",
        ".ai_design/new_frontend_implementation/parity/inventory/drafts.md",
      ]),
    ).toEqual([]);
  });
});

describe("parseArgs", () => {
  it("reads --base in both spellings, --full, and passes the rest to vitest", () => {
    expect(parseArgs(["--base", "abc", "--coverage"])).toEqual({ base: "abc", full: false, vitestArgs: ["--coverage"] });
    expect(parseArgs(["--base=HEAD^1", "--full"])).toEqual({ base: "HEAD^1", full: true, vitestArgs: [] });
    expect(parseArgs([])).toEqual({ base: null, full: false, vitestArgs: [] });
  });

  it("refuses --base without a ref", () => {
    expect(() => parseArgs(["--base"])).toThrow("--base needs a ref");
  });
});

describe("planRun", () => {
  it("runs only affected tests for an ordinary change", () => {
    expect(planRun({ base: "b1", full: false, vitestArgs: ["--coverage"], changedFiles: ["apps/web_new/src/a.ts"] })).toEqual({
      mode: "changed",
      reasons: [],
      args: ["run", "--changed", "b1", "--passWithNoTests", "--coverage"],
    });
  });

  it("runs the whole suite when a full-suite input changed", () => {
    expect(planRun({ base: "b1", full: false, vitestArgs: [], changedFiles: ["src/x.ts", "pnpm-lock.yaml"] })).toEqual({
      mode: "full",
      reasons: ["pnpm-lock.yaml"],
      args: ["run"],
    });
  });

  it("runs the whole suite on --full", () => {
    expect(planRun({ base: "b1", full: true, vitestArgs: ["--coverage"], changedFiles: [] })).toEqual({
      mode: "full",
      reasons: ["--full"],
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
    expect(calls).toEqual(["rev-parse --show-toplevel", "-C /repo diff --name-only base1", "-C /repo ls-files --others --exclude-standard"]);
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
    const status = main(["--coverage"], { git, runVitest: (args) => (runs.push(args), 0), log: (line) => logs.push(line) });
    expect(status).toBe(0);
    expect(calls[0]).toBe("merge-base HEAD origin/web-new-dev");
    expect(runs).toEqual([["run", "--changed", "mb123", "--passWithNoTests", "--coverage"]]);
    expect(logs).toEqual(["test:changed: tests affected by 1 file(s) changed since mb123"]);
  });

  it("uses an explicit base, reports why it runs the whole suite, and returns vitest's status", () => {
    const { git, calls } = fakeGit({ diff: "apps/web_new/vitest.config.ts" });
    const logs = [];
    const status = main(["--base", "HEAD^1"], { git, runVitest: () => 1, log: (line) => logs.push(line) });
    expect(status).toBe(1);
    expect(calls).not.toContain("merge-base HEAD origin/web-new-dev");
    expect(logs).toEqual(["test:changed: full suite (apps/web_new/vitest.config.ts)"]);
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
    expect(runs[0][0]).toBe("run");
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
