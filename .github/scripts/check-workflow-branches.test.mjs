// Tests for check-workflow-branches.mjs.
// Run: npm ci --prefix .github/scripts && node --test .github/scripts/check-workflow-branches.test.mjs
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
  const yml =
    "on:\n  pull_request:\n    branches:\n      - main\n      - 'master'\n  push:\n    branches-ignore:\n      - master # old\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 5, branch: "master" },
    { line: 8, branch: "master" },
  ]);
});

test("flags a scalar branch filter", () => {
  assert.deepEqual(findBannedBranches("on:\n  push:\n    branches: master\n"), [{ line: 3, branch: "master" }]);
});

test("ignores master outside branch filters and other branch names", () => {
  const yml = "on:\n  push:\n    branches: [main, master-docs]\njobs:\n  a:\n    steps:\n      - run: echo master\n";
  assert.deepEqual(findBannedBranches(yml), []);
});

test("flags a filter whose key is quoted", () => {
  const yml =
    "\"on\":\n  'push':\n    \"branches\": [master]\n  pull_request:\n    'branches-ignore':\n      - master\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "master" },
    { line: 6, branch: "master" },
  ]);
});

test("flags master in a flow list that spans several lines", () => {
  const yml = "on:\n  push:\n    branches: [\n      main,\n      master\n    ]\n";
  assert.deepEqual(findBannedBranches(yml), [{ line: 5, branch: "master" }]);
});

test("ignores branch-filter text inside a run script", () => {
  const yml =
    "on:\n  push:\n    branches: [main]\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cat <<EOF\n          branches: [master]\n          EOF\n";
  assert.deepEqual(findBannedBranches(yml), []);
});

test("ignores a branches key outside the on: triggers", () => {
  const yml = "on: [push]\njobs:\n  a:\n    uses: ./.github/workflows/x.yml\n    with:\n      branches: master\n";
  assert.deepEqual(findBannedBranches(yml), []);
});

test("flags glob and negated patterns that name master, not bare globs", () => {
  const yml = "on:\n  push:\n    branches: ['master*', '!master', '**', 'release/*']\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "master*" },
    { line: 3, branch: "!master" },
  ]);
});

test("accepts on: as a plain string or list of events", () => {
  assert.deepEqual(findBannedBranches("on: push\n"), []);
  assert.deepEqual(findBannedBranches("on: [push, pull_request]\n"), []);
  assert.deepEqual(findBannedBranches("on:\n  workflow_dispatch:\n  push:\n"), []);
});

test("no workflow in .github/workflows lists master in its branch filters", () => {
  const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "workflows");
  assert.deepEqual(checkDir(dir), []);
});

test("resolves YAML aliases in branch filters", () => {
  const yml =
    "on:\n  push:\n    branches: &old [master]\n  pull_request:\n    branches: *old\n  merge_group:\n    branches-ignore: &one master\n  workflow_run:\n    branches: [main, *one]\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "master" },
    { line: 5, branch: "master" },
    { line: 7, branch: "master" },
    { line: 9, branch: "master" },
  ]);
});

test("resolves aliased events and filter maps", () => {
  const yml = "on:\n  push: &ev\n    branches: [master]\n  pull_request: *ev\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "master" },
    { line: 4, branch: "master" },
  ]);
});

test("uses GitHub filter semantics: ? and + apply to the preceding character", () => {
  const yml = "on:\n  push:\n    branches: ['masterx?', 'mas+ter', 'mainx?', 'release.?']\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "masterx?" },
    { line: 3, branch: "mas+ter" },
  ]);
});

test("flags globs that match master but not main, even without the literal name", () => {
  const yml = "on:\n  push:\n    branches: ['*ster', 'm[a]ster', 'ma*', '*', 'm*n']\n";
  assert.deepEqual(findBannedBranches(yml), [
    { line: 3, branch: "*ster" },
    { line: 3, branch: "m[a]ster" },
  ]);
});

test("CodeQL runs on pushes and pull requests to main", async () => {
  const { readFileSync } = await import("node:fs");
  const { parse } = await import("yaml");
  const file = join(dirname(fileURLToPath(import.meta.url)), "..", "workflows", "codeql.yml");
  const { on } = parse(readFileSync(file, "utf8"), { version: "1.2" });
  for (const event of ["push", "pull_request"]) {
    assert.ok(on[event]?.branches?.includes("main"), `codeql.yml ${event} must target main`);
  }
});
