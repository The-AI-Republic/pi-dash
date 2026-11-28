// Tests for check-release-version.mjs.
// Run: npm ci --prefix .github/scripts && node --test .github/scripts/check-release-version.test.mjs
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
  const { parse } = await import("yaml");
  const wf = join(dirname(script), "..", "workflows");
  assert.equal(existsSync(join(wf, "check-version.yml")), false, "check-version.yml must not run on every PR");
  const { jobs } = parse(readFileSync(join(wf, "build-branch.yml"), "utf8"));
  // The check runs in the setup job, before anything is built or published.
  const step = jobs.branch_build_setup.steps.find((s) => /check-release-version\.mjs/.test(s.run ?? ""));
  assert.ok(step, "branch_build_setup must run check-release-version.mjs");
  assert.match(step.run, /BUILD_TYPE.*!=.*"Release"/, "the check must be skipped only for non-release builds");
  for (const [name, job] of Object.entries(jobs)) {
    if (name === "branch_build_setup") continue;
    assert.ok([job.needs].flat().includes("branch_build_setup"), `${name} must wait for the version check`);
  }
});
