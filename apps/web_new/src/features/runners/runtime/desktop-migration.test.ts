// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The desktop migration (design §13, steps 1–5) against a fake host and
// server: blocked until the daemon is the app's, then merge, re-add, finish —
// and running it again changes nothing.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    mergeDevMachineInto: vi.fn(),
    getRunner: vi.fn(),
  };
});

import { getRunner, mergeDevMachineInto, type ApiClient } from "@pidash/api-client";

import type { AgentRuntimeApi, DaemonDiscovery, DaemonLocalRunner, LegacyTree } from "../../../core/platform/index.js";
import { migrateLegacyTrees, type MigrationDeps } from "./desktop-migration.js";

const LEGACY_MACHINE = "legacy-machine-1";
const DAEMON_MACHINE = "daemon-machine-1";
const CLOUD = "https://pidash.test";

function compatible(workspaces: { slug: string; signed_in: boolean; runners: number }[]): DaemonDiscovery {
  return {
    kind: "compatible",
    hello: {
      daemon_version: "0.2.0",
      ipc_version: 6,
      cloud_url: CLOUD,
      account: { user_id: "u-1", cloud_url: CLOUD },
      workspaces,
      machine_id: DAEMON_MACHINE,
      engine: "available",
    },
  };
}

function legacyTree(): LegacyTree {
  return {
    workspace: "acme",
    dir: "/app/managed/pidash/acme",
    cloud_url: CLOUD,
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
  };
}

/** A fake computer: the legacy trees on disk and the daemon's runner blocks. */
function fakeComputer(options: {
  discovery: DaemonDiscovery | null;
  runners?: DaemonLocalRunner[];
  podDispatch?: Record<string, string>;
}) {
  let trees: LegacyTree[] = [legacyTree()];
  const runners: DaemonLocalRunner[] = [...(options.runners ?? [])];
  let discovery = options.discovery;
  const signedIn = new Set<string>();
  const log: string[] = [];
  const recorded: unknown[] = [];
  const mergeMock = vi.mocked(mergeDevMachineInto);
  const getRunnerMock = vi.mocked(getRunner);
  mergeMock.mockImplementation(async (_client: ApiClient, source: string, target: string) => {
    log.push(`merge ${source} -> ${target}`);
    return {
      source_dev_machine_id: source,
      target_dev_machine_id: target,
      moved_runner_ids: [],
      collisions: [],
    };
  });
  getRunnerMock.mockImplementation(async (_client: ApiClient, runnerId: string) => {
    const dispatch = options.podDispatch?.[runnerId] ?? "pinned_only";
    return { id: runnerId, pod_dispatch: dispatch } as never;
  });
  const runtime = {
    scanLegacyTrees: vi.fn(async () => trees),
    localRunners: vi.fn(async () => runners),
    addEngineRunner: vi.fn(async (input: { workspace: string; project: string; workingDir?: string }) => {
      const added: DaemonLocalRunner = {
        name: `builtin-${input.project}`,
        runner_id: `id-${input.project}`,
        workspace_slug: input.workspace,
        project_slug: input.project,
        working_dir: input.workingDir ?? "",
        agent: "engine",
      };
      runners.push(added);
      log.push(`add ${input.workspace}/${input.project} ${input.workingDir}`);
      return {
        runner: added.name,
        runner_id: added.runner_id,
        workspace: input.workspace,
        project: input.project,
        working_dir: added.working_dir,
      };
    }),
    restoreLegacyFolder: vi.fn(async (input: { runner: string; workingDir: string }) => {
      log.push(`restore ${input.runner} ${input.workingDir}`);
      const found = runners.find((r) => r.name === input.runner);
      if (found) found.working_dir = input.workingDir;
    }),
    finishLegacyTree: vi.fn(async (input: { dir: string }) => {
      log.push(`finish ${input.dir}`);
      trees = trees.filter((t) => t.dir !== input.dir);
    }),
    recordNotices: vi.fn(async (_account: string, notices: unknown) => {
      recorded.push(notices);
      log.push("record");
      return { notices, dismissed: [] } as never;
    }),
  } as unknown as AgentRuntimeApi;
  const deps: MigrationDeps = {
    runtime,
    client: { baseUrl: CLOUD } as unknown as ApiClient,
    cloudUrl: CLOUD,
    discovery: () => discovery,
    signInWorkspace: vi.fn(async (slug: string) => {
      log.push(`sign_in ${slug}`);
      signedIn.add(slug);
      discovery = compatible([...signedIn].map((s) => ({ slug: s, signed_in: true, runners: 0 })));
    }),
  };
  return { deps, log, recorded };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("desktop migration", () => {
  it("is blocked, touching nothing, until the daemon is the app's", async () => {
    for (const blocked of [
      null,
      { kind: "no_daemon", cli: { installed: false, path: null, version: null, updatable: false } },
      { kind: "account_mismatch", daemon_account: "x@example.com", daemon_cloud_url: "", hosts_runners: true },
    ] as (DaemonDiscovery | null)[]) {
      const computer = fakeComputer({ discovery: blocked });
      // eslint-disable-next-line no-await-in-loop -- one blocked state at a time
      const outcome = await migrateLegacyTrees("u-1", computer.deps);
      expect(outcome.status).toBe("blocked");
      expect(computer.log).toEqual([]);
    }
  });

  it("signs the workspace in, merges, re-adds each runner with its folder, then finishes", async () => {
    const computer = fakeComputer({ discovery: compatible([]) });
    const outcome = await migrateLegacyTrees("u-1", computer.deps);
    expect(outcome).toEqual({ status: "done", migrated: ["acme"], failed: [] });
    expect(computer.log).toEqual([
      "sign_in acme",
      `merge ${LEGACY_MACHINE} -> ${DAEMON_MACHINE}`,
      "add acme/WEB /app/managed/workdirs/acme/WEB",
      "add acme/API /app/managed/workdirs/acme/API",
      "finish /app/managed/pidash/acme",
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
    expect(computer.log.filter((line) => line.startsWith("add"))).toHaveLength(2);
  });

  it("is idempotent: a second run finds nothing to do", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]),
    });
    await migrateLegacyTrees("u-1", computer.deps);
    const before = computer.log.length;
    await expect(migrateLegacyTrees("u-1", computer.deps)).resolves.toEqual({ status: "nothing_to_do" });
    expect(computer.log.length).toBe(before);
  });

  it("does not sign in a workspace the daemon already holds", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]),
    });
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.log.some((line) => line.startsWith("sign_in"))).toBe(false);
  });

  it("records no notice when nothing collided and no engine became a pod member", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]),
    });
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.recorded).toEqual([]);
  });

  it("names the legacy folder left unused when the daemon already ran the engine", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 1 }]),
      runners: [
        {
          name: "builtin-WEB",
          runner_id: "daemon-web",
          workspace_slug: "acme",
          project_slug: "WEB",
          working_dir: "/home/u/pidash/acme/WEB",
          agent: "engine",
        },
      ],
    });
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.log.filter((line) => line.startsWith("add"))).toEqual([
      "add acme/API /app/managed/workdirs/acme/API",
    ]);
    expect(computer.recorded).toEqual([
      {
        legacy_folders: [{ workspace: "acme", project: "WEB", path: "/app/managed/workdirs/acme/WEB" }],
        pod_member_projects: [],
      },
    ]);
  });

  it("gives a runner the reconcile re-added with a fresh folder its legacy folder back, with no notice", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 1 }]),
      runners: [
        {
          name: "builtin-mac-acme-web",
          runner_id: "r-1",
          workspace_slug: "acme",
          project_slug: "WEB",
          working_dir: "/home/u/pidash/acme/WEB",
          agent: "engine",
        },
      ],
    });
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.log).toContain("restore builtin-mac-acme-web /app/managed/workdirs/acme/WEB");
    expect(computer.log.filter((line) => line.startsWith("add"))).toHaveLength(1);
    expect(computer.recorded).toEqual([]);
  });

  it("matches the daemon's engine to a legacy block whatever the project's letter case", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 1 }]),
      runners: [
        {
          name: "builtin-web",
          runner_id: "daemon-web",
          workspace_slug: "acme",
          project_slug: "web",
          working_dir: "/home/u/pidash/acme/web",
          agent: "engine",
        },
      ],
    });
    const outcome = await migrateLegacyTrees("u-1", computer.deps);
    expect(outcome).toEqual({ status: "done", migrated: ["acme"], failed: [] });
    expect(computer.log.filter((line) => line.startsWith("add"))).toEqual([
      "add acme/API /app/managed/workdirs/acme/API",
    ]);
  });

  it("names the projects whose engine the server made a pod member (PO-11), before finishing", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]),
      podDispatch: { "id-API": "pod_member" },
    });
    const runtime = computer.deps.runtime as unknown as {
      recordNotices: (account: string, notices: unknown, ...rest: unknown[]) => Promise<unknown>;
      finishLegacyTree: (input: { dir: string }, ...rest: unknown[]) => Promise<unknown>;
    };
    const order: string[] = [];
    const origRecord = runtime.recordNotices.bind(runtime);
    const origFinish = runtime.finishLegacyTree.bind(runtime);
    runtime.recordNotices = async (...args: unknown[]) => {
      order.push("record");
      return origRecord(...(args as [string, unknown]));
    };
    runtime.finishLegacyTree = async (...args: unknown[]) => {
      order.push("finish");
      return origFinish(...(args as [{ dir: string }]));
    };
    await migrateLegacyTrees("u-1", computer.deps);
    expect(computer.recorded).toEqual([
      { legacy_folders: [], pod_member_projects: [{ runner_id: "id-API", workspace: "acme", project: "API" }] },
    ]);
    expect(order).toEqual(["record", "finish"]);
  });

  it("does not fail the tree when the pod lookup fails", async () => {
    const computer = fakeComputer({
      discovery: compatible([{ slug: "acme", signed_in: true, runners: 0 }]),
    });
    vi.mocked(getRunner).mockRejectedValueOnce(new Error("offline"));
    await expect(migrateLegacyTrees("u-1", computer.deps)).resolves.toEqual({
      status: "done",
      migrated: ["acme"],
      failed: [],
    });
  });
});
