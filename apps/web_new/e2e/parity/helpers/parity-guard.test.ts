// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for stack/parity-guard.sh (NEWFRONT-132): the ownership + port
// preflight behind parity-up.sh / parity-reset.sh. Docker itself is faked
// (a stub on PATH answering `compose ps -a --format json` with canned
// NDJSON), so these tests never touch a real stack; port occupancy is real
// (a held TCP port), because bind behavior is exactly what is asserted.
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const guard = join(dirname(fileURLToPath(import.meta.url)), "..", "stack", "parity-guard.sh");

interface GuardResult {
  status: number;
  stdout: string;
  stderr: string;
}

function runGuardRaw(args: string[], psOutput: string): GuardResult {
  const binDir = mkdtempSync(join(realpathSync(tmpdir()), "parity-guard-bin-"));
  const stub = join(binDir, "docker");
  writeFileSync(stub, "#!/usr/bin/env bash\nprintf '%s' \"$FAKE_DOCKER_PS\"\n");
  chmodSync(stub, 0o755);
  const path = process.env["PATH"] === undefined ? binDir : `${binDir}:${process.env["PATH"]}`;
  const result = spawnSync("bash", [guard, ...args], {
    encoding: "utf8",
    env: { ...process.env, PATH: path, FAKE_DOCKER_PS: psOutput },
  });
  return { status: result.status ?? 1, stdout: String(result.stdout ?? ""), stderr: String(result.stderr ?? "") };
}

function makeStackDir(): string {
  return mkdtempSync(join(realpathSync(tmpdir()), "parity-stack-"));
}

function psLine(entry: unknown): string {
  return JSON.stringify(entry);
}

function labelsFor(workingDir: string): string {
  return `com.docker.compose.project.working_dir=${workingDir},com.docker.compose.service=pg`;
}

/** Hold a TCP port open for the duration of fn; the guard must read it as busy. */
async function withHeldPort(fn: (port: number) => void): Promise<void> {
  const server = createServer();
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  expect(port).toBeGreaterThan(0);
  try {
    fn(port);
  } finally {
    await new Promise<void>((resolve) => {
      server.close(() => resolve());
    });
  }
}

/** A port that is free right now (tiny race, as with any bind probe). */
async function freePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  await new Promise<void>((resolve) => {
    server.close(() => resolve());
  });
  expect(port).toBeGreaterThan(0);
  return port;
}

describe("parity-guard.sh usage", () => {
  it("exits 0 for an empty project with no ports requested", () => {
    const result = runGuardRaw(["parity257", makeStackDir()], "");
    expect(result.status).toBe(0);
  });

  it("exits 2 on an unknown argument", () => {
    const result = runGuardRaw(["parity257", makeStackDir(), "--bogus"], "");
    expect(result.status).toBe(2);
    expect(result.stderr).toContain("unknown argument");
  });

  it("exits non-zero when --ports has no value", () => {
    const result = runGuardRaw(["parity257", makeStackDir(), "--ports"], "");
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("--ports needs a value");
  });

  it("exits non-zero with no arguments", () => {
    expect(runGuardRaw([], "").status).not.toBe(0);
  });

  it("ignores blank and malformed ps lines", () => {
    const result = runGuardRaw(["parity257", makeStackDir()], "\nnot json\n  \n");
    expect(result.status).toBe(0);
  });
});

describe("parity-guard.sh ownership", () => {
  it("refuses (exit 3) a project owned by another checkout", () => {
    const foreign = makeStackDir();
    const own = makeStackDir();
    const result = runGuardRaw(["parity257", own], `${psLine({ Labels: labelsFor(foreign), Publishers: [] })}\n`);
    expect(result.status).toBe(3);
    expect(result.stderr).toContain("another checkout");
    expect(result.stderr).toContain(foreign);
    expect(result.stderr).toContain("Nothing was built, started or destroyed");
  });

  it("fails closed when a container carries no compose labels", () => {
    const result = runGuardRaw(["parity257", makeStackDir()], `${psLine({ Labels: "", Publishers: [] })}\n`);
    expect(result.status).toBe(3);
    expect(result.stderr).toContain("<unknown>");
  });

  it("accepts this checkout's own containers", () => {
    const own = makeStackDir();
    const result = runGuardRaw(["parity257", own], `${psLine({ Labels: labelsFor(own), Publishers: [] })}\n`);
    expect(result.status).toBe(0);
  });

  it("warns instead of refusing with --warn", () => {
    const foreign = makeStackDir();
    const own = makeStackDir();
    const result = runGuardRaw(
      ["parity257", own, "--warn"],
      `${psLine({ Labels: labelsFor(foreign), Publishers: [] })}\n`
    );
    expect(result.status).toBe(0);
    expect(result.stderr).toContain("another checkout");
    expect(result.stderr).toContain("Proceeding anyway");
  });
});

describe("parity-guard.sh ports", () => {
  it("accepts a free port", async () => {
    const port = await freePort();
    const result = runGuardRaw(["parity257", makeStackDir(), "--ports", String(port)], "");
    expect(result.status).toBe(0);
  });

  it("exempts this checkout's own published ports", async () => {
    await withHeldPort((port) => {
      const own = makeStackDir();
      const ps = psLine({
        Labels: labelsFor(own),
        // An entry without a published port (unpublished) must be skipped.
        Publishers: [{}, { URL: "0.0.0.0", TargetPort: 5432, PublishedPort: port, Protocol: "tcp" }],
      });
      const result = runGuardRaw(["parity257", own, "--ports", String(port)], `${ps}\n`);
      expect(result.status).toBe(0);
    });
  });

  it("refuses (exit 1) a port held by someone else", { timeout: 30_000 }, async () => {
    await withHeldPort((port) => {
      const result = runGuardRaw(["parity257", makeStackDir(), "--ports", String(port)], "");
      expect(result.status).toBe(1);
      expect(result.stderr).toContain("already in use");
    });
  });

  it("treats a non-numeric port as busy without probing", () => {
    const result = runGuardRaw(["parity257", makeStackDir(), "--ports", "abc"], "");
    expect(result.status).toBe(1);
    expect(result.stderr).toContain("abc");
  });
});
