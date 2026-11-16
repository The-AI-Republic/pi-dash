// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the license-header check: generated coverage/ output must not
// be scanned (lcov ships its own CSS without our header), while hand-written
// source without a header must still fail.
import { afterAll, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { isCliEntry, main, runHeaderCheck } from "./check-headers.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "check-headers.mjs");

const HEADER = [
  "Copyright (c) Pi Dash contributors",
  "SPDX-License-Identifier: AGPL-3.0-only",
  "See the LICENSE file for details.",
].join("\n");
const OLD_COPYRIGHT = ["Copyright (c) 2023-present", "Pi Dash Software, Inc. and contributors"].join(" ");
const tempRoots = [];

function makeRepo(files) {
  const root = mkdtempSync(join(tmpdir(), "header-check-"));
  tempRoots.push(root);
  for (const [rel, content] of Object.entries(files)) {
    const full = join(root, rel);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
  }
  return root;
}

afterAll(() => {
  for (const root of tempRoots) rmSync(root, { recursive: true, force: true });
});

describe("runHeaderCheck", () => {
  it("ignores generated coverage/ output and still flags headerless source", () => {
    const root = makeRepo({
      "apps/web_new/coverage/lcov-report/base.css": "body { margin: 0; }",
      "apps/web_new/src/ok.ts": `${HEADER}\nexport const ok = 1;\n`,
      "packages/kit/src/missing.ts": "export const missing = 1;\n",
    });

    const failures = runHeaderCheck({ root, trees: ["apps/web_new", "packages/kit"] });

    expect(failures.some((failure) => failure.includes("coverage/"))).toBe(false);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("packages/kit/src/missing.ts");
  });

  it("walks nested directories and skips generated files in every tree", () => {
    const root = makeRepo({
      "apps/web_new/src/features/deep/row.tsx": `${HEADER}\nexport const row = 1;\n`,
      "packages/api-client/src/deep/missing.ts": "export const missing = 1;\n",
      "packages/api-client/coverage/lcov-report/base.css": "body {}",
      "packages/kit/src/routeTree.gen.ts": "export const generated = 1;\n",
      "packages/kit/dist/generated.css": "body {}",
    });

    const failures = runHeaderCheck({ root, trees: ["apps/web_new", "packages/api-client", "packages/kit"] });

    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("packages/api-client/src/deep/missing.ts");
  });

  it("flags a file carrying the old tree's copyright line", () => {
    const root = makeRepo({
      "apps/web_new/src/copied.ts": `${OLD_COPYRIGHT}\n${HEADER}\nexport const copied = 1;\n`,
    });

    const failures = runHeaderCheck({ root, trees: ["apps/web_new"] });

    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("carries the old tree's copyright line");
  });

  it("tolerates a missing tree so an optional package can be absent", () => {
    const root = makeRepo({});

    expect(runHeaderCheck({ root, trees: ["apps/web_new"] })).toEqual([]);
  });

  it("reports an ENOENT below the tree root instead of skipping the whole tree", () => {
    const root = makeRepo({
      "apps/web_new/src/ok.ts": `${HEADER}\nexport const ok = 1;\n`,
      "apps/web_new/src/noheader.ts": "export const noheader = 1;\n",
    });
    symlinkSync(join(root, "apps/web_new/src/does-not-exist.ts"), join(root, "apps/web_new/src/dangling.ts"));

    expect(() => runHeaderCheck({ root, trees: ["apps/web_new"] })).toThrow(/ENOENT/);
  });

  it("surfaces a non-ENOENT read error instead of treating it as a missing tree", () => {
    const root = makeRepo({ "apps/web_new/src": "not a directory" });

    expect(() => runHeaderCheck({ root, trees: ["apps/web_new/src"] })).toThrow(/ENOTDIR/);
  });

  it("surfaces a non-ENOENT error while probing the tree root", () => {
    const root = makeRepo({ "apps/web_new/src": "not a directory" });

    expect(() => runHeaderCheck({ root, trees: ["apps/web_new/src/child"] })).toThrow(/ENOTDIR/);
  });

  it("main returns 0 for a clean tree and 1 for a headerless file", () => {
    const clean = makeRepo({ "apps/web_new/src/ok.ts": `${HEADER}\nexport const ok = 1;\n` });
    const dirty = makeRepo({ "apps/web_new/src/bad.ts": "export const bad = 1;\n" });

    expect(main({ root: clean, trees: ["apps/web_new"] })).toBe(0);
    expect(main({ root: dirty, trees: ["apps/web_new"] })).toBe(1);
  });

  it("runs as a CLI when invoked directly", () => {
    expect(isCliEntry(script)).toBe(true);
    expect(isCliEntry(join(dirname(script), "other.mjs"))).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
    expect(() => execFileSync(process.execPath, [script], { encoding: "utf8" })).not.toThrow();
  });

  it("prints a failure line for each offending file", () => {
    const dirty = makeRepo({
      "apps/web_new/src/one.ts": "export const one = 1;\n",
      "apps/web_new/src/two.ts": "export const two = 2;\n",
    });

    const failures = runHeaderCheck({ root: dirty, trees: ["apps/web_new"] });

    expect(failures).toHaveLength(2);
    expect(failures.join("\n")).toContain("apps/web_new/src/one.ts");
    expect(failures.join("\n")).toContain("apps/web_new/src/two.ts");
  });
});
