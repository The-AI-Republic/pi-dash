// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the parity disk guard (run: pnpm --filter web_new
// test:parity-disk-check). Stubs df(1) and docker(1) on PATH so every
// threshold branch is exercised without touching the real host disk.
import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const checker = join(here, "parity-disk-check.sh");

const DF_STUB = [
  "#!/bin/sh",
  'if [ "${DF_FAIL:-}" = "${2:-}" ] && [ -n "${DF_FAIL:-}" ]; then exit 1; fi',
  'if [ "${DF_GARBAGE:-0}" = "1" ]; then',
  '  echo "Filesystem 1024-blocks Used Available Capacity Mounted on"',
  '  echo "garbage line without numbers"',
  "  exit 0",
  "fi",
  'case "${2:-}" in',
  '  /) pct="${DF_ROOT_PCT:-50}" ;;',
  '  /extra) pct="${DF_EXTRA_PCT:-50}" ;;',
  '  *) pct="${DF_DOCKER_PCT:-50}" ;;',
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
  // The script asks for OperatingSystem|DockerRootDir first; a separate
  // format request (the VM measurement's container) gets df-like output.
  'case "$*" in',
  "  *run*)",
  '    if [ "${VM_DF_FAIL:-0}" = "1" ]; then exit 1; fi',
  '    vm_pct="${VM_DISK_PCT:-50}"',
  "    vm_used=$((vm_pct * 1000))",
  "    vm_avail=$(((100 - vm_pct) * 1000))",
  '    echo "Filesystem 1024-blocks Used Available Capacity Mounted on"',
  '    echo "overlay 100000 $vm_used $vm_avail ${vm_pct}% /"',
  "    exit 0 ;;",
  "esac",
  'echo "${DOCKER_OS:-}|${DOCKER_ROOT_DIR:-/var/lib/docker}"',
  "",
].join("\n");

const UNAME_STUB = [
  "#!/bin/sh",
  'case "$1" in',
  '-s) echo "${UNAME_S:-Darwin}" ;;',
  '*) echo "${UNAME_S:-Darwin}" ;;',
  "esac",
  "",
].join("\n");

function stubBin() {
  const bin = mkdtempSync(join(tmpdir(), "parity-disk-bin-"));
  writeFileSync(join(bin, "df"), DF_STUB, { mode: 0o755 });
  writeFileSync(join(bin, "docker"), DOCKER_STUB, { mode: 0o755 });
  writeFileSync(join(bin, "uname"), UNAME_STUB, { mode: 0o755 });
  return bin;
}

const text = (value) => (value == null ? "" : String(value));

function runChecker(bin, { env = {}, args = [] } = {}) {
  const result = spawnSync(checker, args, {
    env: {
      ...process.env,
      PATH: `${bin}${delimiter}${process.env.PATH}`,
      ...env,
    },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  return {
    status: result.status,
    stdout: text(result.stdout),
    stderr: text(result.stderr),
  };
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
    const dataRoot = mkdtempSync(join(tmpdir(), "parity-docker-root-"));
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_PCT: "50", DF_DOCKER_PCT: "95", DOCKER_ROOT_DIR: dataRoot },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, new RegExp(`${dataRoot} at 95% used`));
  });

  it("measures a VM-backed daemon's disk inside the daemon instead of failing", () => {
    // Docker Desktop: the data-root only exists inside the daemon VM, so
    // the host df cannot read it. The guard must measure the VM disk.
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_PCT: "20",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DISK_PCT: "42",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /measured inside the daemon/);
    assert.match(result.stdout, /\/vm\/no\/such\/docker-root .* at 42% used/);
    assert.match(result.stdout, /disk check ok: worst 42% used on \/vm\/no\/such\/docker-root/);
  });

  it("applies the same refusal threshold to the VM disk", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_PCT: "50",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DISK_PCT: "94",
      },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /REFUSED: \/vm\/no\/such\/docker-root at 94% used/);
  });

  it("warns and continues when the VM disk cannot be measured", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_PCT: "50",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DF_FAIL: "1",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stderr, /VM disk was not checked, host \/ check only/);
    assert.match(result.stdout, /disk check ok/);
  });

  it("still exits 2 for a genuinely bad data-root on a Linux host", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_PCT: "50",
        DOCKER_OS: "Debian GNU/Linux 12 (bookworm)",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        UNAME_S: "Linux",
      },
    });
    assert.equal(result.status, 2);
    assert.match(result.stderr, /cannot read disk usage for '\/vm\/no\/such\/docker-root'/);
  });

  it("keeps the host df check when the data-root exists on the host", () => {
    const dataRoot = mkdtempSync(join(tmpdir(), "parity-docker-root-"));
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_PCT: "50",
        DF_DOCKER_PCT: "93",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: dataRoot,
      },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, new RegExp(`${dataRoot} at 93% used`));
    assert.doesNotMatch(result.stdout, /measured inside the daemon/);
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

  it("prints usage for --help and -h", () => {
    const bin = stubBin();
    for (const flag of ["--help", "-h"]) {
      const result = runChecker(bin, { args: [flag] });
      assert.equal(result.status, 0);
      assert.match(result.stdout, /usage: parity-disk-check\.sh/);
    }
  });

  it("rejects non-numeric and out-of-range thresholds", () => {
    const bin = stubBin();
    const warnNaN = runChecker(bin, {
      env: { PARITY_DISK_WARN_PCT: "eighty" },
    });
    assert.equal(warnNaN.status, 2);
    assert.match(warnNaN.stderr, /PARITY_DISK_WARN_PCT must be an integer/);
    for (const value of ["0", "101"]) {
      const warnRange = runChecker(bin, {
        env: { PARITY_DISK_WARN_PCT: value },
      });
      assert.equal(warnRange.status, 2);
      assert.match(warnRange.stderr, /integers 1-100/);
      const maxRange = runChecker(bin, {
        env: { PARITY_DISK_MAX_USE_PCT: value },
      });
      assert.equal(maxRange.status, 2);
      assert.match(maxRange.stderr, /integers 1-100/);
    }
  });

  it("rejects a warn threshold equal to the refuse threshold", () => {
    const result = runChecker(stubBin(), {
      env: { PARITY_DISK_WARN_PCT: "90", PARITY_DISK_MAX_USE_PCT: "90" },
    });
    assert.equal(result.status, 2);
    assert.match(result.stderr, /must be below/);
  });

  it("treats a docker data-root of / as the same filesystem", () => {
    const bin = stubBin();
    const healthy = runChecker(bin, {
      env: { DF_ROOT_PCT: "50", DOCKER_ROOT_DIR: "/" },
    });
    assert.equal(healthy.status, 0);
    assert.match(healthy.stdout, /same filesystem/);
    const full = runChecker(bin, {
      env: { DF_ROOT_PCT: "95", DOCKER_ROOT_DIR: "/" },
    });
    assert.equal(full.status, 1);
    assert.match(full.stdout, /\/ at 95% used/);
  });

  it("exits 2 when df fails or reports garbage", () => {
    const bin = stubBin();
    const failed = runChecker(bin, { env: { DF_FAIL: "/" } });
    assert.equal(failed.status, 2);
    assert.match(failed.stderr, /cannot read disk usage/);
    const garbage = runChecker(bin, { env: { DF_GARBAGE: "1" } });
    assert.equal(garbage.status, 2);
    assert.match(garbage.stderr, /cannot read disk usage/);
  });
});
