// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the changed-tests runner: how the selection becomes a vitest
// run, which changes are reported as untested, and how the changed-file list
// is gathered. test-impact.test.mjs covers the selection itself.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

import {
  changedFilesSince,
  defaultReadBase,
  isCliEntry,
  main,
  parseArgs,
  planRun,
  skippedFiles,
} from "./test-changed.mjs";

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
  it("runs exactly the selected test files, as absolute paths", () => {
    expect(
      planRun({ full: false, vitestArgs: ["--coverage"], tests: ["apps/web_new/src/a.test.ts"], root: "/repo" })
    ).toEqual({
      mode: "changed",
      reason: null,
      args: ["run", "--passWithNoTests", "--coverage", "/repo/apps/web_new/src/a.test.ts"],
    });
  });

  it("runs nothing when nothing was selected", () => {
    expect(planRun({ full: false, vitestArgs: [], tests: [], root: "/repo" })).toEqual({
      mode: "none",
      reason: null,
      args: null,
    });
  });

  it("runs the whole suite when too many test files were selected for one command line", () => {
    const tests = Array.from({ length: 1001 }, (_, i) => `apps/web_new/src/f${i}.test.ts`);
    expect(planRun({ full: false, vitestArgs: ["--coverage"], tests, root: "/repo" })).toEqual({
      mode: "full",
      reason: "1001 test files selected",
      args: ["run", "--coverage"],
    });
  });

  it("runs the whole suite on --full", () => {
    expect(planRun({ full: true, vitestArgs: ["--coverage"], tests: [], root: "" })).toEqual({
      mode: "full",
      reason: "--full",
      args: ["run", "--coverage"],
    });
  });
});

// A fake git that answers the commands the runner issues.
function fakeGit({ diff = "", untracked = "", tracked = "", base = {} } = {}) {
  const calls = [];
  const git = (args) => {
    calls.push(args.join(" "));
    if (args[0] === "rev-parse") return "/repo";
    if (args[0] === "merge-base") return "mb123";
    if (args[2] === "diff") return diff;
    if (args[2] === "ls-files") return args.includes("--cached") ? tracked : untracked;
    if (args[2] === "show") {
      const file = args[3].slice(args[3].indexOf(":") + 1);
      if (file in base) return base[file];
      throw new Error(`fatal: path '${file}' does not exist`);
    }
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
  const tree = {
    "packages/kit/src/index.ts": 'export { Button } from "./Button";\nexport { Avatar } from "./Avatar";',
    "packages/kit/src/Button.tsx": "export const Button = 1;",
    "packages/kit/src/Avatar.tsx": "export const Avatar = 1;",
    "apps/web_new/src/uses-button.test.tsx": 'import { Button } from "@pidash/kit";',
    "apps/web_new/src/uses-avatar.test.tsx": 'import { Avatar } from "@pidash/kit";',
  };
  const tracked = Object.keys(tree).join("\n");
  const read = (file) => tree[file] ?? null;
  const baseReads = [];
  const readBaseFiles = (root, base, files) => (
    baseReads.push([root, base, files]), new Map(files.map((file) => [file, tree[file] ?? null]))
  );
  const env = {};

  it("defaults the base to the merge base, runs the tests the change reaches and returns vitest's status", () => {
    const { git, calls } = fakeGit({ diff: "packages/kit/src/Button.tsx", tracked });
    const runs = [];
    const logs = [];
    const status = main(["--coverage"], {
      git,
      runVitest: (args) => (runs.push(args), 1),
      log: (line) => logs.push(line),
      read,
      readBaseFiles,
      env,
    });
    expect(status).toBe(1);
    expect(calls[0]).toBe("merge-base HEAD origin/web-new-dev");
    expect(baseReads).toEqual([["/repo", "mb123", ["packages/kit/src/Button.tsx"]]]);
    expect(runs).toEqual([["run", "--passWithNoTests", "--coverage", "/repo/apps/web_new/src/uses-button.test.tsx"]]);
    expect(logs).toEqual(["test:changed: 1 file(s) changed since mb123 reach 1 test file(s)"]);
  });

  it("uses an explicit base, reports untested inputs and runs nothing when no test is reached", () => {
    const { git, calls } = fakeGit({ diff: "apps/web_new/vitest.config.ts", tracked });
    const runs = [];
    const logs = [];
    const status = main(["--base", "HEAD^1"], {
      git,
      runVitest: (args) => (runs.push(args), 1),
      log: (line) => logs.push(line),
      read,
      readBaseFiles,
      env,
    });
    expect(status).toBe(0);
    expect(calls).not.toContain("merge-base HEAD origin/web-new-dev");
    expect(runs).toEqual([]);
    expect(logs).toEqual([
      "test:changed: 1 file(s) changed since HEAD^1 reach 0 test file(s)",
      "test:changed: no import edge, left to the scheduled full suite: apps/web_new/vitest.config.ts",
    ]);
  });

  it("reads the working tree from disk by default", () => {
    const root = mkdtempSync(join(tmpdir(), "test-changed-"));
    for (const [file, text] of Object.entries(tree)) {
      mkdirSync(join(root, dirname(file)), { recursive: true });
      writeFileSync(join(root, file), text);
    }
    const fake = fakeGit({ diff: "packages/kit/src/Avatar.tsx", tracked: `${tracked}\napps/web_new/src/gone.ts` });
    const git = (args) => (args[0] === "rev-parse" ? root : fake.git(args));
    const runs = [];
    main([], { git, runVitest: (args) => (runs.push(args), 0), log: () => {}, env });
    expect(runs).toEqual([["run", "--passWithNoTests", join(root, "apps/web_new/src/uses-avatar.test.tsx")]]);
  });

  it("reports why it runs the whole suite when too many test files are reached", () => {
    const many = Object.fromEntries(
      Array.from({ length: 1001 }, (_, i) => [
        `apps/web_new/src/t${i}.test.ts`,
        'import { Button } from "@pidash/kit";',
      ])
    );
    const big = { ...tree, ...many };
    const logs = [];
    const runs = [];
    main([], {
      git: fakeGit({ diff: "packages/kit/src/Button.tsx", tracked: Object.keys(big).join("\n") }).git,
      runVitest: (args) => (runs.push(args), 0),
      log: (line) => logs.push(line),
      read: (file) => big[file] ?? null,
      readBaseFiles,
      env,
    });
    expect(logs).toEqual(["test:changed: full suite (1002 test files selected since mb123)"]);
    expect(runs).toEqual([["run"]]);
  });

  it("says when the change reaches the test setup file", () => {
    const setup = { ...tree, "apps/web_new/src/test-setup.ts": 'import { Button } from "@pidash/kit";' };
    const logs = [];
    main([], {
      git: fakeGit({ diff: "packages/kit/src/Button.tsx", tracked: Object.keys(setup).join("\n") }).git,
      runVitest: () => 0,
      log: (line) => logs.push(line),
      read: (file) => setup[file] ?? null,
      readBaseFiles,
      env,
    });
    expect(logs.at(-1)).toBe(
      "test:changed: the change reaches src/test-setup.ts, which every test loads; left to the scheduled full suite"
    );
  });

  it("resolves @pidash/edition the way vitest does", () => {
    const edition = {
      ...tree,
      "apps/web_new/src/core/edition/cloud.ts": "export const edition = 2;",
      "apps/web_new/src/edition.test.ts": 'import { edition } from "@pidash/edition";',
    };
    const runs = [];
    main([], {
      git: fakeGit({ diff: "apps/web_new/src/core/edition/cloud.ts", tracked: Object.keys(edition).join("\n") }).git,
      runVitest: (args) => (runs.push(args), 0),
      log: () => {},
      read: (file) => edition[file] ?? null,
      readBaseFiles,
      env: { PIDASH_EDITION_MODULE: join(process.cwd(), "src/core/edition/oss.ts") },
    });
    expect(runs).toEqual([]);
  });

  it("skips the diff entirely on --full", () => {
    const { git, calls } = fakeGit();
    const runs = [];
    main(["--full", "--base", "x"], { git, runVitest: (args) => (runs.push(args), 0), log: () => {} });
    expect(calls).toEqual([]);
    expect(runs).toEqual([["run"]]);
  });
});

describe("defaultReadBase", () => {
  it("reads each file at the base in one batch, null where it did not exist", () => {
    const root = mkdtempSync(join(tmpdir(), "test-changed-git-"));
    const git = (...args) => execFileSync("git", ["-C", root, ...args], { stdio: "ignore" });
    git("init", "-q");
    mkdirSync(join(root, "src"));
    writeFileSync(join(root, "src/a.ts"), "export const a = 1;\n");
    writeFileSync(join(root, "src/é.ts"), "export const é = 'ü';\n");
    git("add", ".");
    git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base");
    expect(defaultReadBase(root, "HEAD", ["src/a.ts", "src/new.ts", "src/é.ts"])).toEqual(
      new Map([
        ["src/a.ts", "export const a = 1;\n"],
        ["src/new.ts", null],
        ["src/é.ts", "export const é = 'ü';\n"],
      ])
    );
    expect(defaultReadBase(root, "HEAD", [])).toEqual(new Map());
  });

  it("returns nothing when git cannot answer", () => {
    expect(defaultReadBase(mkdtempSync(join(tmpdir(), "not-a-repo-")), "HEAD", ["a.ts"])).toEqual(new Map());
  });
});

describe("main with the real git", () => {
  it("diffs this checkout against an explicit base", () => {
    const logs = [];
    expect(main(["--base", "HEAD"], { runVitest: () => 0, log: (line) => logs.push(line) })).toBe(0);
    expect(logs[0]).toMatch(/^test:changed: \d+ file\(s\) changed since HEAD reach \d+ test file\(s\)$/);
  });
});

describe("isCliEntry", () => {
  it("is false when imported or without an entry script", () => {
    expect(isCliEntry("/somewhere/else.mjs")).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
  });
});
