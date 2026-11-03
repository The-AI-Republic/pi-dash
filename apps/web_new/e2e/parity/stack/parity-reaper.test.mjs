// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression test for parity-reaper.sh (run: pnpm --filter web_new
// test:parity-stack). NEWFRONT-350: `arm` used to background `setsid` without
// --fork, so the intermediate shell blocked in waitpid until the run ended
// and held the bring-up pipe open, hanging any `parity-up.sh ... | tail`.
// arm must now exit promptly, leave exactly one fully-detached watcher that
// is tracked in its MARK file, and still tear the stack down when the agent
// exits. Linux-only (/proc, setsid); uses scratch PARITY_PROJECT names and a
// private TMPDIR, and only ever signals its own scratch processes.
import { strict as assert } from "node:assert";
import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const reaper = join(here, "parity-reaper.sh");

const linuxOnly = process.platform !== "linux";
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function bashPath() {
  const found = spawnSync("bash", ["-c", "command -v bash"], { encoding: "utf8" });
  const path = found.stdout?.trim().split("\n")[0];
  assert.ok(found.status === 0 && path, "bash is required for the reaper test");
  return path;
}

// One process's argv plus environ, or null when it vanished mid-read.
function readProc(pid) {
  try {
    const argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0").filter(Boolean);
    const env = readFileSync(`/proc/${pid}/environ`, "utf8").split("\0").filter(Boolean);
    return { pid: Number(pid), argv, env };
  } catch {
    return null;
  }
}

function allProcs() {
  const out = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    const proc = readProc(entry);
    if (proc) out.push(proc);
  }
  return out;
}

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

describe("parity-reaper.sh", { skip: linuxOnly ? "Linux-only: needs /proc and setsid" : false }, () => {
  let workdir = "";
  let pidash = "";
  let scratch = "";
  let childEnv = {};
  // A long-lived shell under a `pidash`-named ancestor: agent_pid() in any
  // arm it runs resolves to this shell, so the watcher cannot fire mid-test
  // (its 15s loop only starts counting once the agent below dies).
  let repl = null;
  let replOut = "";
  let replErr = "";
  let armedAgent = 0;
  let armedWatcher = 0;
  let armedMark = "";
  const extraPids = [];

  before(() => {
    workdir = mkdtempSync(join(tmpdir(), "parity-reaper-test-"));
    pidash = join(workdir, "pidash");
    copyFileSync(bashPath(), pidash);
    chmodSync(pidash, 0o755);
    scratch = `parityreapertest${process.pid}`;
    childEnv = { ...process.env, TMPDIR: workdir, PARITY_PROJECT: scratch };
    // `bash; exit` keeps the pidash comm: a lone `bash` would be exec'd and
    // the ancestor would no longer look like the runner daemon.
    repl = spawn(pidash, ["-c", "bash; exit 0"], {
      env: childEnv,
      stdio: ["pipe", "pipe", "pipe"],
    });
    repl.stdout.on("data", (chunk) => {
      replOut += chunk.toString();
    });
    repl.stderr.on("data", (chunk) => {
      replErr += chunk.toString();
    });
  });

  after(() => {
    try {
      repl?.kill("SIGKILL");
    } catch {
      // Already gone.
    }
    for (const p of mine()) {
      try {
        process.kill(p.pid, "SIGKILL");
      } catch {
        // Already gone.
      }
    }
    for (const pid of extraPids) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        // Already gone.
      }
    }
    if (workdir) rmSync(workdir, { recursive: true, force: true });
  });

  // Only this test's scratch processes: never touch siblings on a shared box.
  function mine() {
    return allProcs().filter((p) => p.env.includes(`PARITY_PROJECT=${scratch}`));
  }

  function isArm(p) {
    return (
      p.argv.length === 3 && p.argv[0].endsWith("bash") && p.argv[1].endsWith("parity-reaper.sh") && p.argv[2] === "arm"
    );
  }

  function watchersFor(agent) {
    return mine().filter(
      (p) =>
        p.argv.length === 4 &&
        p.argv[0].endsWith("bash") &&
        p.argv[1].endsWith("parity-reaper.sh") &&
        p.argv[2] === "watch" &&
        p.argv[3] === String(agent)
    );
  }

  // Run one command in the repl and return its stdout. The repl's pipes stay
  // open across commands, like `parity-up.sh ... | tail`: a pre-fix arm
  // leaves an intermediate holding them, which the assertions below catch.
  async function runRepl(command) {
    const sentinel = `__REAPER_RC_${Date.now()}_${Math.floor(Math.random() * 1e6)}__`;
    const seen = replOut.length;
    repl.stdin.write(`${command}; echo "${sentinel}=$?"\n`);
    const deadline = Date.now() + 10000;
    for (;;) {
      const fresh = replOut.slice(seen);
      const match = new RegExp(`^${sentinel}=(\\d+)$`, "m").exec(fresh);
      if (match) {
        return { code: Number(match[1]), out: fresh.slice(0, match.index) };
      }
      if (repl.exitCode !== null || Date.now() > deadline) {
        assert.fail(`repl command timed out or repl died (exit=${repl.exitCode}): ${command}\nstderr: ${replErr}`);
      }
      await sleep(25);
    }
  }

  async function armOnce() {
    const started = Date.now();
    const { code, out } = await runRepl(`bash "${reaper}" arm`);
    assert.equal(code, 0, `arm exits 0, output: ${JSON.stringify(out)}`);
    assert.ok(Date.now() - started < 10000, "arm returns promptly");
    return /when this run \(pid (\d+)\)/.exec(out)?.[1] ?? null;
  }

  it("arms outside a pidash run only when an ancestor exists", () => {
    // Direct invocation: a no-op outside a Pi Dash run (CI), a real arm on
    // a runner box (whose daemon ancestor agent_pid finds). Either way the
    // outcome must be self-consistent and leave no residue behind.
    const probeProject = `${scratch}n`;
    const result = spawnSync("bash", [reaper, "arm"], {
      env: { ...childEnv, PARITY_PROJECT: probeProject },
      encoding: "utf8",
      timeout: 15000,
    });
    assert.equal(result.status, 0, `direct arm exits 0, stderr: ${result.stderr}`);
    const agent = /when this run \(pid (\d+)\)/.exec(result.stdout)?.[1];
    const marks = readdirSync(workdir).filter((f) => f.startsWith(`parity-reaper.${probeProject}.`));
    if (!agent) {
      assert.deepEqual(marks, [], "silent no-op creates no MARK file");
      return;
    }
    assert.equal(marks.length, 1, "an armed run records exactly one MARK file");
    const watcher = Number(readFileSync(join(workdir, marks[0]), "utf8").trim());
    assert.ok(isAlive(watcher), "the MARK pid is a live process");
    extraPids.push(watcher);
    process.kill(watcher, "SIGKILL");
    rmSync(join(workdir, marks[0]));
  });

  it("exits promptly with exactly one detached, tracked watcher", async () => {
    const agent = await armOnce();
    assert.ok(agent, "the first arm reports its agent pid");
    armedAgent = Number(agent);
    const mark = join(workdir, `parity-reaper.${scratch}.${armedAgent}.pid`);
    assert.ok(existsSync(mark), "arm records its MARK file");
    const watcher = Number(readFileSync(mark, "utf8").trim());
    assert.ok(Number.isInteger(watcher) && watcher > 0, "MARK holds a pid");
    assert.ok(isAlive(watcher), "the MARK pid is a live process");
    armedWatcher = watcher;
    armedMark = mark;

    const argv = readProc(watcher)?.argv ?? [];
    assert.deepEqual(
      argv.slice(0, 3).map((a) => a.split("/").pop()),
      ["bash", "parity-reaper.sh", "watch"],
      "MARK tracks the watcher itself, not a spawn intermediate"
    );
    assert.equal(argv[3], String(armedAgent), "the watcher guards this run's agent");
    assert.deepEqual(
      watchersFor(armedAgent).map((p) => p.pid),
      [watcher],
      "exactly one watcher"
    );
    assert.deepEqual(
      mine()
        .filter(isArm)
        .map((p) => p.pid),
      [],
      "no lingering arm intermediates (the NEWFRONT-350 hang)"
    );

    const fds = [0, 1, 2].map((fd) => readlinkSync(`/proc/${watcher}/fd/${fd}`));
    assert.deepEqual(fds, ["/dev/null", "/dev/null", "/dev/null"], "watcher holds no pipes");
    assert.equal(readlinkSync(`/proc/${watcher}/cwd`), "/", "watcher keeps cwd /");
    const watcherEnv = readFileSync(`/proc/${watcher}/environ`, "utf8").split("\0");
    assert.ok(watcherEnv.includes(`PARITY_PROJECT=${scratch}`), "watcher targets this stack");
  });

  it("a second arm is a MARK no-op", async () => {
    assert.ok(armedWatcher > 0, "the first arm ran (test order)");
    assert.ok(isAlive(armedWatcher), "the first watcher is still up");
    const agent = await armOnce();
    assert.equal(agent, null, "re-arm prints nothing: it exits on the live MARK");
    assert.equal(
      Number(readFileSync(armedMark, "utf8").trim()),
      armedWatcher,
      "re-arm keeps the live watcher instead of spawning again"
    );
    assert.deepEqual(
      watchersFor(armedAgent).map((p) => p.pid),
      [armedWatcher],
      "still one watcher"
    );
  });

  it("the watcher tears down and exits once its agent is gone", async () => {
    assert.ok(armedWatcher > 0, "the first arm ran (test order)");
    repl.stdin.end();
    const replGoneBy = Date.now() + 5000;
    while (repl.exitCode === null && Date.now() < replGoneBy) {
      await sleep(50);
    }
    assert.ok(repl.exitCode !== null, "closing stdin ends the fake agent");
    // The 15s watch loop fires on its own against the scratch project (an
    // empty compose project: a no-op down).
    const deadline = Date.now() + 35000;
    while ((isAlive(armedWatcher) || existsSync(armedMark)) && Date.now() < deadline) {
      await sleep(1000);
    }
    assert.ok(!isAlive(armedWatcher), "the watcher exits after its agent");
    assert.ok(!existsSync(armedMark), "the watcher removes its MARK file");
  });
});
