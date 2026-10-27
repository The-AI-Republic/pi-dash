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
  // Free space in GB per filesystem; size defaults to 100 GB.
  'case "${2:-}" in',
  '  /) free="${DF_ROOT_FREE:-50}" ;;',
  '  /extra) free="${DF_EXTRA_FREE:-50}" ;;',
  '  *) free="${DF_DOCKER_FREE:-50}" ;;',
  "esac",
  'size=$((${DF_SIZE_GB:-100} * 1048576))',
  "avail=$((free * 1048576))",
  "used=$((size - avail))",
  "pct=$((used * 100 / size))",
  'echo "Filesystem 1024-blocks Used Available Capacity Mounted on"',
  'echo "/dev/fake $size $used $avail ${pct}% ${2:-/}"',
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
  '    if [ "${VM_DF_GARBAGE:-0}" = "1" ]; then echo "garbage"; exit 0; fi',
  "    vm_size=$((100 * 1048576))",
  '    vm_avail=$((${VM_DISK_FREE:-50} * 1048576))',
  "    vm_used=$((vm_size - vm_avail))",
  "    vm_pct=$((vm_used * 100 / vm_size))",
  '    echo "Filesystem 1024-blocks Used Available Capacity Mounted on"',
  '    echo "overlay $vm_size $vm_used $vm_avail ${vm_pct}% /"',
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
    const result = runChecker(stubBin(), { env: { DF_ROOT_FREE: "0" } });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /REFUSED/);
    assert.match(result.stdout, /\/ has 0\.0G free, 100% used/);
  });

  it("refuses just below the default 6 GB floor and passes at it", () => {
    const bin = stubBin();
    const below = runChecker(bin, { env: { DF_ROOT_FREE: "5" } });
    assert.equal(below.status, 1);
    assert.match(below.stdout, /REFUSED: \/ has 5\.0G free/);
    assert.match(below.stdout, /needs 6G free/);
    const at = runChecker(bin, { env: { DF_ROOT_FREE: "6" } });
    assert.equal(at.status, 0);
    assert.match(at.stdout, /WARNING/);
  });

  it("passes a large disk at 90% used that still has plenty free (NEWFRONT-851)", () => {
    const result = runChecker(stubBin(), {
      env: { DF_SIZE_GB: "1900", DF_ROOT_FREE: "190", DOCKER_DOWN: "1" },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /disk check ok: least free 190\.0G \(90% used\)/);
  });

  it("warns but proceeds between the warn and refuse floors", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_FREE: "10" } });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /WARNING: \/ has 10\.0G free/);
  });

  it("passes a healthy disk", () => {
    const result = runChecker(stubBin(), { env: { DF_ROOT_FREE: "55" } });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /disk check ok/);
  });

  it("honors PARITY_DISK_MIN_FREE_GB and PARITY_DISK_WARN_FREE_GB overrides", () => {
    const bin = stubBin();
    const relaxed = runChecker(bin, {
      env: { DF_ROOT_FREE: "3", PARITY_DISK_MIN_FREE_GB: "2", PARITY_DISK_WARN_FREE_GB: "4" },
    });
    assert.equal(relaxed.status, 0);
    assert.match(relaxed.stdout, /WARNING/);
    const strict = runChecker(bin, {
      env: { DF_ROOT_FREE: "20", PARITY_DISK_MIN_FREE_GB: "25", PARITY_DISK_WARN_FREE_GB: "40" },
    });
    assert.equal(strict.status, 1);
  });

  it("bypasses the refusal only via the documented escape hatch", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_FREE: "0", PARITY_IGNORE_DISK_CHECK: "1" },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /BYPASSED/);
  });

  it("also refuses when only Docker's data-root filesystem is full", () => {
    const dataRoot = mkdtempSync(join(tmpdir(), "parity-docker-root-"));
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_FREE: "50", DF_DOCKER_FREE: "2", DOCKER_ROOT_DIR: dataRoot },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, new RegExp(`REFUSED: ${dataRoot} has 2\\.0G free`));
  });

  it("judges by the filesystem with the least free space", () => {
    const dataRoot = mkdtempSync(join(tmpdir(), "parity-docker-root-"));
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_FREE: "40", DF_DOCKER_FREE: "30", DOCKER_ROOT_DIR: dataRoot },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, new RegExp(`least free 30\\.0G \\(70% used\\) on ${dataRoot}`));
  });

  it("measures a VM-backed daemon's disk inside the daemon instead of failing", () => {
    // Docker Desktop: the data-root only exists inside the daemon VM, so
    // the host df cannot read it. The guard must measure the VM disk.
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "80",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DISK_FREE: "58",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /measured inside the daemon/);
    assert.match(result.stdout, /\/vm\/no\/such\/docker-root .* at 42% used/);
    assert.match(result.stdout, /disk check ok: least free 58\.0G \(42% used\) on \/vm\/no\/such\/docker-root/);
  });

  it("applies the same refusal floor to the VM disk", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "50",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DISK_FREE: "4",
      },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /REFUSED: \/vm\/no\/such\/docker-root has 4\.0G free/);
  });

  it("warns but proceeds when the VM disk is between the floors", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "50",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DISK_FREE: "12",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /WARNING: \/vm\/no\/such\/docker-root has 12\.0G free/);
    assert.match(result.stdout, /measured inside the daemon/);
  });

  it("treats Darwin as VM-backed even when OperatingSystem is not Docker Desktop", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "50",
        DOCKER_OS: "Debian GNU/Linux 12 (bookworm)",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        UNAME_S: "Darwin",
        VM_DISK_FREE: "67",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /measured inside the daemon/);
    assert.match(result.stdout, /at 33% used/);
  });

  it("warns and continues when the in-VM df output is unparseable", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "50",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: "/vm/no/such/docker-root",
        VM_DF_GARBAGE: "1",
      },
    });
    assert.equal(result.status, 0);
    assert.match(result.stderr, /VM disk was not checked, host \/ check only/);
    assert.match(result.stdout, /disk check ok/);
  });

  it("warns and continues when the VM disk cannot be measured", () => {
    const result = runChecker(stubBin(), {
      env: {
        DF_ROOT_FREE: "50",
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
        DF_ROOT_FREE: "50",
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
        DF_ROOT_FREE: "50",
        DF_DOCKER_FREE: "3",
        DOCKER_OS: "Docker Desktop",
        DOCKER_ROOT_DIR: dataRoot,
      },
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, new RegExp(`${dataRoot} has 3\\.0G free`));
    assert.doesNotMatch(result.stdout, /measured inside the daemon/);
  });

  it("falls back to / when docker is unreachable", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_FREE: "50", DOCKER_DOWN: "1" },
    });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /checked \/ only/);
  });

  it("covers extra --path filesystems", () => {
    const result = runChecker(stubBin(), {
      env: { DF_ROOT_FREE: "50", DF_EXTRA_FREE: "4", DOCKER_DOWN: "1" },
      args: ["--path", "/extra"],
    });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /\/extra has 4\.0G free/);
  });

  it("rejects bad floors and bad arguments with exit 2", () => {
    const bin = stubBin();
    const badValue = runChecker(bin, { env: { PARITY_DISK_MIN_FREE_GB: "soon" } });
    assert.equal(badValue.status, 2);
    assert.match(badValue.stderr, /PARITY_DISK_MIN_FREE_GB must be a whole number/);
    const badWarn = runChecker(bin, { env: { PARITY_DISK_WARN_FREE_GB: "lots" } });
    assert.equal(badWarn.status, 2);
    assert.match(badWarn.stderr, /PARITY_DISK_WARN_FREE_GB must be a whole number/);
    const zero = runChecker(bin, { env: { PARITY_DISK_MIN_FREE_GB: "0" } });
    assert.equal(zero.status, 2);
    assert.match(zero.stderr, /at least 1/);
    const inverted = runChecker(bin, {
      env: { PARITY_DISK_WARN_FREE_GB: "6", PARITY_DISK_MIN_FREE_GB: "10" },
    });
    assert.equal(inverted.status, 2);
    assert.match(inverted.stderr, /must be above/);
    const equal = runChecker(bin, {
      env: { PARITY_DISK_WARN_FREE_GB: "8", PARITY_DISK_MIN_FREE_GB: "8" },
    });
    assert.equal(equal.status, 2);
    const badFlag = runChecker(bin, { args: ["--rebuild"] });
    assert.equal(badFlag.status, 2);
    const missingValue = runChecker(bin, { args: ["--path"] });
    assert.equal(missingValue.status, 2);
  });

  it("refuses the retired percentage overrides instead of ignoring them", () => {
    const bin = stubBin();
    for (const name of ["PARITY_DISK_MAX_USE_PCT", "PARITY_DISK_WARN_PCT"]) {
      const result = runChecker(bin, { env: { DF_ROOT_FREE: "50", [name]: "95" } });
      assert.equal(result.status, 2);
      assert.match(result.stderr, new RegExp(`${name} is no longer supported`));
    }
  });

  it("prints usage for --help and -h", () => {
    const bin = stubBin();
    for (const flag of ["--help", "-h"]) {
      const result = runChecker(bin, { args: [flag] });
      assert.equal(result.status, 0);
      assert.match(result.stdout, /usage: parity-disk-check\.sh/);
    }
  });

  it("treats a docker data-root of / as the same filesystem", () => {
    const bin = stubBin();
    const healthy = runChecker(bin, {
      env: { DF_ROOT_FREE: "50", DOCKER_ROOT_DIR: "/" },
    });
    assert.equal(healthy.status, 0);
    assert.match(healthy.stdout, /same filesystem/);
    const full = runChecker(bin, {
      env: { DF_ROOT_FREE: "5", DOCKER_ROOT_DIR: "/" },
    });
    assert.equal(full.status, 1);
    assert.match(full.stdout, /\/ has 5\.0G free, 95% used/);
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
