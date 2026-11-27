// Tests for check-release-version.mjs.
// Run: node --test .github/scripts/check-release-version.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { releaseVersionError } from "./check-release-version.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "check-release-version.mjs");

test("accepts a platform tag that matches package.json", () => {
  assert.equal(releaseVersionError("platform-v1.4.0", "1.4.0"), null);
  assert.equal(releaseVersionError("v1.4.0", "1.4.0"), null);
  assert.equal(releaseVersionError("platform-v1.4.0-rc.1", "1.4.0-rc.1"), null);
});

test("rejects a tag that does not match package.json", () => {
  assert.match(releaseVersionError("platform-v1.4.0", "1.3.0"), /1\.4\.0.*1\.3\.0/);
  assert.match(releaseVersionError("platform-v1.4.0-rc.1", "1.4.0"), /1\.4\.0-rc\.1/);
});

test("rejects a missing version on either side", () => {
  assert.ok(releaseVersionError("", "1.4.0"));
  assert.ok(releaseVersionError("platform-v1.4.0", undefined));
});

test("CLI exits 1 on mismatch and 0 on match", () => {
  const dir = mkdtempSync(join(tmpdir(), "relver-"));
  const pkg = join(dir, "package.json");
  writeFileSync(pkg, JSON.stringify({ version: "1.3.0" }));
  const bad = spawnSync(process.execPath, [script, "platform-v1.4.0", pkg], { encoding: "utf8" });
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /1\.3\.0/);
  const ok = spawnSync(process.execPath, [script, "platform-v1.3.0", pkg], { encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stderr);
});

test("check-version no longer gates ordinary PRs; the platform release checks the tag", async () => {
  const { readFileSync, existsSync } = await import("node:fs");
  const wf = join(dirname(script), "..", "workflows");
  assert.equal(existsSync(join(wf, "check-version.yml")), false, "check-version.yml must not run on every PR");
  const build = readFileSync(join(wf, "build-branch.yml"), "utf8");
  assert.match(build, /check-release-version\.mjs/);
});
