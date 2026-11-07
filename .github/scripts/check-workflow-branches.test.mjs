// Tests for check-workflow-branches.mjs. Run: node --test .github/scripts/
import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { findBannedBranches, checkDir } from "./check-workflow-branches.mjs";

test("flags master in an inline branch list", () => {
  const yml = 'on:\n  push:\n    branches: ["preview", "master"]\n';
  assert.deepEqual(findBannedBranches(yml), [{ line: 3, branch: "master" }]);
});

test("flags master in a block branch list, quoted or not", () => {
  const yml = "on:\n  pull_request:\n    branches:\n      - main\n      - 'master'\n  push:\n    branches-ignore:\n      - master # old\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 5, branch: "master" },
    { line: 8, branch: "master" },
  ]);
});

test("flags a scalar branch filter", () => {
  assert.deepEqual(findBannedBranches("on:\n  push:\n    branches: master\n"), [{ line: 3, branch: "master" }]);
});

test("ignores master outside branch filters and other branch names", () => {
  const yml = 'on:\n  push:\n    branches: [main, master-docs]\njobs:\n  a:\n    steps:\n      - run: echo master\n';
  assert.deepEqual(findBannedBranches(yml), []);
});

test("no workflow in .github/workflows lists master in its branch filters", () => {
  const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "workflows");
  assert.deepEqual(checkDir(dir), []);
});
