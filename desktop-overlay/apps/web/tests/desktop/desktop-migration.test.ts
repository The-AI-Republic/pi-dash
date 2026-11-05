/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The desktop migration (design §13, steps 1–5) against a fake host and
 * server: blocked until the daemon is the app's, then merge, re-add, finish —
 * and running it again changes nothing.
 */

import { describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "https://pidash.test" }));
vi.mock("@/pi-dash-web/components/desktop/agent-runtime-edition", () => ({
  CSRF_TOKEN_PATH: "/api/auth/get-csrf-token/",
  AGENT_RUNTIME_REASON_MESSAGES: {},
}));
vi.mock("@pi-dash/services", () => ({
  APIService: class {
    request = vi.fn();
  },
}));

import { mergePath, migrateLegacyTrees, type LegacyTree, type MigrationDeps } from "@/services/desktop-migration";
import type { Discovery, LocalRunner } from "@/services/desktop-daemon";

const LEGACY_MACHINE = "legacy-machine-1";
const DAEMON_MACHINE = "daemon-machine-1";

function compatible(workspaces: { slug: string; signed_in: boolean; runners: number }[]): Discovery {
  return {
    kind: "compatible",
    hello: {
      daemon_version: "0.2.0",
      ipc_version: 6,
      cloud_url: "https://pidash.test",
      account: { user_id: "u-1", cloud_url: "https://pidash.test" },
      workspaces,
      machine_id: DAEMON_MACHINE,
      engine: "available",
    },
  };
}

/** A fake computer: the legacy trees on disk and the daemon's runner blocks. */
function fakeComputer(options: { discovery: Discovery | null; workspaces?: string[] }) {
  let trees: LegacyTree[] = [
    {
      workspace: "acme",
      dir: "/app/managed/pidash/acme",
      cloud_url: "https://pidash.test",
      dev_machine_id: LEGACY_MACHINE,
      engine_runners: [
        {
          name: "desktop-desktop-x-WEB",
          runner_id: "r-1",
          project: "WEB",
          working_dir: "/app/managed/workdirs/acme/WEB",
        },
        {
          name: "desktop-desktop-x-API",
          runner_id: "r-2",
          project: "API",
          working_dir: "/app/managed/workdirs/acme/API",
        },
      ],
    },
  ];
  const runners: LocalRunner[] = [];
  let discovery = options.discovery;
  const signedIn = new Set(options.workspaces ?? []);
  const log: string[] = [];
  const deps: MigrationDeps = {
    discovery: () => discovery,
    signInWorkspace: async (slug) => {
      log.push(`sign_in ${slug}`);
      signedIn.add(slug);
      discovery = compatible([...signedIn].map((s) => ({ slug: s, signed_in: true, runners: 0 })));
    },
    post: async <T>(path: string, body: unknown) => {
      log.push(`POST ${path} ${JSON.stringify(body)}`);
      return {} as T;
    },
    invoke: async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "legacy_scan") return trees as T;
      if (command === "daemon_runners") return runners as T;
      log.push(`${command} ${JSON.stringify(args ?? {})}`);
      if (command === "daemon_engine_runner_add") {
        runners.push({
          name: `builtin-${String(args?.project)}`,
          runner_id: `id-${String(args?.project)}`,
          workspace_slug: String(args?.workspace),
          project_slug: String(args?.project),
          working_dir: String(args?.workingDir),
          agent: "engine",
        });
      }
      if (command === "legacy_finish") trees = trees.filter((t) => t.dir !== args?.dir);
      return undefined as T;
    },
  };
  return { deps, log, setDiscovery: (d: Discovery | null) => (discovery = d) };
}

describe("desktop migration", () => {
  it("is blocked, touching nothing, until the daemon is the app's", async () => {
    for (const blocked of [
      null,
      { kind: "no_daemon", cli: { installed: false, path: null, version: null, updatable: false } },
      { kind: "account_mismatch", daemon_account: "x@example.com", daemon_cloud_url: "", hosts_runners: true },
    ] as (Discovery | null)[]) {
      const computer = fakeComputer({ discovery: blocked });
      // eslint-disable-next-line no-await-in-loop -- one blocked state at a time
      const outcome = await migrateLegacyTrees("u-1", computer.deps);
      expect(outcome.status).toBe("blocked");
      expect(computer.log).toEqual([]);
    }
  });

  it("signs the workspace in, merges the legacy machine, re-adds each engine runner with its folder, then finishes", async () => {
    const computer = fakeComputer({ discovery: compatible([]) });
    const outcome = await migrateLegacyTrees("u-1", computer.deps);
    expect(outcome).toEqual({ status: "done", migrated: ["acme"], failed: [] });
    expect(computer.log).toEqual([
      "sign_in acme",
      `POST ${mergePath(LEGACY_MACHINE)} {"target_dev_machine_id":"${DAEMON_MACHINE}"}`,
      'daemon_engine_runner_add {"workspace":"acme","project":"WEB","workingDir":"/app/managed/workdirs/acme/WEB"}',
      'daemon_engine_runner_add {"workspace":"acme","project":"API","workingDir":"/app/managed/workdirs/acme/API"}',
      'legacy_finish {"dir":"/app/managed/pidash/acme","account":"u-1","daemonAccount":"u-1"}',
    ]);
  });

  it("joins a run already in flight instead of starting a second one", async () => {
    const computer = fakeComputer({ discovery: compatible([]) });
    const [a, b] = await Promise.all([
      migrateLegacyTrees("u-1", computer.deps),
      migrateLegacyTrees("u-1", computer.deps),
    ]);
    expect(a).toEqual({ status: "done", migrated: ["acme"], failed: [] });
    expect(b).toBe(a);
    expect(computer.log.filter((line) => line.startsWith("sign_in"))).toHaveLength(1);
    expect(computer.log.filter((line) => line.startsWith("daemon_engine_runner_add"))).toHaveLength(2);
  });

  it("is idempotent: a second run finds nothing to do", async () => {
    const computer = fakeComputer({ discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]) });
    await migrateLegacyTrees("u-1", computer.deps);
    const before = computer.log.length;
    await expect(migrateLegacyTrees("u-1", computer.deps)).resolves.toEqual({ status: "nothing_to_do" });
    expect(computer.log.length).toBe(before);
  });

  it("resumes a half-finished tree without adding a runner twice", async () => {
    const computer = fakeComputer({ discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]) });
    let failFinish = true;
    const invoke = computer.deps.invoke;
    computer.deps.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "legacy_finish" && failFinish) throw new Error("disk busy");
      return invoke<T>(command, args);
    };
    const first = await migrateLegacyTrees("u-1", computer.deps);
    expect(first).toEqual({ status: "done", migrated: [], failed: [{ workspace: "acme", error: "disk busy" }] });
    failFinish = false;
    await migrateLegacyTrees("u-1", computer.deps);
    const adds = computer.log.filter((line) => line.startsWith("daemon_engine_runner_add"));
    expect(adds).toHaveLength(2);
    expect(computer.log.filter((line) => line.startsWith("legacy_finish"))).toHaveLength(1);
  });

  it("does not sign in a workspace the daemon already holds", async () => {
    const computer = fakeComputer({ discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]) });
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.log.some((line) => line.startsWith("sign_in"))).toBe(false);
  });
});
