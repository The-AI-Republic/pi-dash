// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the parity disk guard (run: pnpm --filter web_new
// test:parity-disk-check). Stubs df(1) and docker(1) on PATH so every
// threshold branch is exercised without touching the real host disk.
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const checker = join(here, "parity-disk-check.sh");

const DF_STUB = [
  "#!/bin/sh",
  'case "${2:-}" in',
  '  /extra) pct="${DF_EXTRA_PCT:-50}" ;;',
  '  /var/lib/docker) pct="${DF_DOCKER_PCT:-50}" ;;',
  '  *) pct="${DF_ROOT_PCT:-50}" ;;',
  "esac",
  "used=$((pct * 1000))",
  "avail=$(((100 - pct) * 1000))",
  'echo "Filesystem 1024-blocks Used Available Capacity Mounted on"',
  'echo "/dev/fake 100000 $used $avail ${pct}% ${2:-/}"',
  "",
].join("\n");

const DOCKER_STUB = [
  "#!/bin/sh",
  'if [ "${DOCKER_DOWN:-0}" = "1" ]; then exit 1; fi',
  'echo "${DOCKER_ROOT_DIR:-/var/lib/docker}"',
  "",
].join("\n");

function stubBin() {
  const bin = mkdtempSync(join(tmpdir(), "parity-disk-bin-"));
  writeFileSync(join(bin, "df"), DF_STUB, { mode: 0o755 });
  writeFileSync(join(bin, "docker"), DOCKER_STUB, { mode: 0o755 });
  return bin;
}

const text = (value) => (value == null ? "" : String(value));

function runChecker(bin, { env = {}, args = [] } = {}) {
  try {
    const stdout = execFileSync(checker, args, {
      env: {
        ...process.env,
        PATH: `${bin}${delimiter}${process.env.PATH}`,
        ...env,
      },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return { status: 0, stdout: text(stdout), stderr: "" };
  } catch (err) {
    return {
      status: err.status,
      stdout: text(err.stdout),
      stderr: text(err.stderr),
    };
  }
}

describe("parity disk guard", () => {
  it("refuses a full root disk like the NEWFRONT-175 incident", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_PCT: "100" } });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /REFUSED/);
    assert.match(result.stdout, /\/ at 100% used/);
  });

  it("refuses exactly at the default limit", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_PCT: "90" } });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /REFUSED/);
  });

  it("warns but proceeds between warn and refuse", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_PCT: "85" } });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /WARNING/);
  });

  it("passes a healthy disk", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_PCT: "45" } });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /disk check ok/);
  });

  it("honors PARITY_DISK_MAX_USE_PCT overrides", () => {
    const bin = stubBin();
    const relaxed = runChecker(bin, {
      env: { DF_ROOT_PCT: "97", PARITY_DISK_MAX_USE_PCT: "99" },
    });
    assert.equal(relaxed.status, 0);
    assert.match(relaxed.stdout, /WARNING/);
    const strict = runChecker(bin, {
      env: { DF_ROOT_PCT: "85", PARITY_DISK_MAX_USE_PCT: "80", PARITY_DISK_WARN_PCT: "70" },
    });
    assert.equal(strict.status, 1);
  });

  it("bypasses the refusal only via the documented escape hatch", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_PCT: "100", PARITY_IGNORE_DISK_CHECK: "1" },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /BYPASSED/);
  });

  it("also refuses when only Docker's data-root filesystem is full", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_PCT: "50", DF_DOCKER_PCT: "95" },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /\/var\/lib\/docker at 95% used/);
  });

  it("falls back to / when docker is unreachable", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_PCT: "50", DOCKER_DOWN: "1" },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /checked \/ only/);
  });

  it("covers extra --path filesystems", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_PCT: "50", DF_EXTRA_PCT: "96", DOCKER_DOWN: "1" },
      args: ["--path", "/extra"],
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /\/extra at 96% used/);
  });

  it("rejects bad thresholds and bad arguments with exit 2", () => {
    const bin = stubBin();
    const badValue = runChecker(bin, { env: { PARITY_DISK_MAX_USE_PCT: "soon" } });
    assert.equal(badValue.status, 2);
    const inverted = runChecker(bin, {
      env: { PARITY_DISK_WARN_PCT: "95", PARITY_DISK_MAX_USE_PCT: "90" },
    });
    assert.equal(inverted.status, 2);
    assert.match(inverted.stderr, /must be below/);
    const badFlag = runChecker(bin, { args: ["--rebuild"] });
    assert.equal(badFlag.status, 2);
    const missingValue = runChecker(bin, { args: ["--path"] });
    assert.equal(missingValue.status, 2);
  });
});
