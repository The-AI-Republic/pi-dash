// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// "Is this <label>? Replace it" (design §4.10) against a fake host, server
// and settings store: which machine is asked about, what confirming does
// (merge into this machine, then re-create the moved engine runners on the
// daemon), and that declining is remembered.

import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    listReplaceCandidates: vi.fn(),
    listWorkspaces: vi.fn(),
    listRunners: vi.fn(),
    mergeDevMachineInto: vi.fn(),
  };
});

import {
  listReplaceCandidates,
  listRunners,
  listWorkspaces,
  mergeDevMachineInto,
  type ApiClient,
} from "@pidash/api-client";

import type { AgentRuntimeApi, DaemonDiscovery, DaemonLocalRunner } from "../../../core/platform/index.js";
import {
  declineReplace,
  errorText,
  findReplacePrompt,
  offerReplace,
  registerReplacePrompt,
  replaceMachine,
  type ReplaceCandidate,
  type ReplaceDeps,
} from "./desktop-replace.js";

const THIS_MACHINE = "machine-new";
const CLOUD = "https://pidash.test";

function compatible(
  machineId = THIS_MACHINE,
  workspaces = [{ slug: "acme", signed_in: true, runners: 0 }]
): DaemonDiscovery {
  return {
    kind: "compatible",
    hello: {
      daemon_version: "0.2.0",
      ipc_version: 6,
      cloud_url: CLOUD,
      account: { user_id: "u-1", cloud_url: CLOUD },
      workspaces,
      machine_id: machineId,
      engine: "available",
    },
  };
}

const candidate = (id: string, extra: Partial<ReplaceCandidate> = {}): ReplaceCandidate => ({
  id,
  label: "rich-laptop",
  host_label: "rich-laptop",
  last_seen_at: "2026-10-30T12:00:00Z",
  provisioning: "manual",
  ...extra,
});

function serverRunner(
  id: string,
  project: string,
  provisioning: "desktop_bundled" | "manual"
): Record<string, unknown> {
  return {
    id,
    provisioning,
    pod_detail: { project_identifier: project },
  };
}

/** A fake world: server candidates and runners, the daemon, the settings file. */
function fakeWorld(
  options: {
    discovery?: DaemonDiscovery | null;
    candidates?: ReplaceCandidate[];
    moved?: string[];
    serverRunners?: Record<string, Record<string, unknown>[]>;
    local?: DaemonLocalRunner[];
  } = {}
) {
  let discovery = options.discovery === undefined ? compatible() : options.discovery;
  let dismissed: string[] = [];
  const log: string[] = [];
  vi.mocked(listReplaceCandidates).mockResolvedValue({ candidates: options.candidates ?? [] } as never);
  vi.mocked(listWorkspaces).mockResolvedValue([
    { id: "ws-acme", slug: "acme" },
    { id: "ws-beta", slug: "beta" },
  ] as never);
  vi.mocked(listRunners).mockImplementation(async (_client: ApiClient, workspaceId: string) => {
    const slug = workspaceId === "ws-acme" ? "acme" : "beta";
    return (options.serverRunners?.[slug] ?? []) as never;
  });
  vi.mocked(mergeDevMachineInto).mockImplementation(async (_client: ApiClient, source: string, target: string) => {
    log.push(`merge ${source} -> ${target}`);
    return {
      source_dev_machine_id: source,
      target_dev_machine_id: target,
      moved_runner_ids: options.moved ?? [],
      collisions: [],
    };
  });
  const local: DaemonLocalRunner[] = [...(options.local ?? [])];
  const runtime = {
    loadNotices: vi.fn(async () => ({
      notices: { legacy_folders: [], pod_member_projects: [] },
      dismissed,
    })),
    dismissNotices: vi.fn(async (_account: string, keys: string[]) => {
      dismissed = [...dismissed, ...keys];
      return { notices: { legacy_folders: [], pod_member_projects: [] }, dismissed };
    }),
    localRunners: vi.fn(async () => local),
    addEngineRunner: vi.fn(async (input: { workspace: string; project: string }) => {
      log.push(`add ${input.workspace}/${input.project}`);
      const added: DaemonLocalRunner = {
        name: `builtin-${input.project}`,
        runner_id: `id-${input.project}`,
        workspace_slug: input.workspace,
        project_slug: input.project,
        working_dir: "/tasks",
        agent: "engine",
      };
      local.push(added);
      return {
        runner: added.name,
        runner_id: added.runner_id,
        workspace: input.workspace,
        project: input.project,
        working_dir: added.working_dir,
      };
    }),
  } as unknown as AgentRuntimeApi;
  const deps: ReplaceDeps = {
    runtime,
    client: { baseUrl: CLOUD } as unknown as ApiClient,
    cloudUrl: CLOUD,
    discovery: () => discovery,
    signInWorkspace: vi.fn(async (slug: string) => {
      log.push(`sign_in ${slug}`);
      discovery = compatible(THIS_MACHINE, [
        { slug: "acme", signed_in: true, runners: 0 },
        { slug, signed_in: true, runners: 0 },
      ]);
    }),
  };
  return { deps, log, dismissed: () => dismissed, setDiscovery: (d: DaemonDiscovery | null) => (discovery = d), local };
}

afterEach(() => {
  vi.clearAllMocks();
  registerReplacePrompt(null);
});

describe("replace-machine prompt", () => {
  it("asks about the most recently seen offline machine with this hostname", async () => {
    const world = fakeWorld({ candidates: [candidate("old-1"), candidate("old-2")] });
    await expect(findReplacePrompt("u-1", world.deps)).resolves.toEqual({
      machineId: THIS_MACHINE,
      candidate: candidate("old-1"),
    });
  });

  it("asks nothing when there is no candidate, or the daemon is not the app's", async () => {
    await expect(findReplacePrompt("u-1", fakeWorld().deps)).resolves.toBeNull();
    const elsewhere = fakeWorld({
      discovery: { kind: "account_mismatch", daemon_account: "x", daemon_cloud_url: "", hosts_runners: false },
      candidates: [candidate("old-1")],
    });
    await expect(findReplacePrompt("u-1", elsewhere.deps)).resolves.toBeNull();
  });

  it("leaves legacy desktop machines to the migration", async () => {
    const world = fakeWorld({ candidates: [candidate("legacy", { provisioning: "desktop_bundled" })] });
    await expect(findReplacePrompt("u-1", world.deps)).resolves.toBeNull();
  });

  it("declining is remembered: that machine is never asked about again", async () => {
    const world = fakeWorld({ candidates: [candidate("old-1"), candidate("old-2")] });
    const prompt = await findReplacePrompt("u-1", world.deps);
    await declineReplace("u-1", prompt!, world.deps);
    expect(world.dismissed()).toEqual(["replace:old-1"]);
    // Nothing was merged.
    expect(world.log).toEqual([]);
    // The next one is offered instead; once both are declined, nothing.
    const next = await findReplacePrompt("u-1", world.deps);
    expect(next?.candidate.id).toBe("old-2");
    await declineReplace("u-1", next!, world.deps);
    await expect(findReplacePrompt("u-1", world.deps)).resolves.toBeNull();
  });

  it("confirming merges the old machine into this one and re-creates its engine runners on the daemon", async () => {
    const world = fakeWorld({
      candidates: [candidate("old-1")],
      moved: ["r-web", "r-own", "r-beta"],
      serverRunners: {
        acme: [
          serverRunner("r-web", "WEB", "desktop_bundled"),
          serverRunner("r-own", "WEB", "manual"),
          serverRunner("unrelated", "OPS", "desktop_bundled"),
        ],
        beta: [serverRunner("r-beta", "API", "desktop_bundled")],
      },
    });
    const prompt = await findReplacePrompt("u-1", world.deps);
    const outcome = await replaceMachine(prompt!, world.deps);
    expect(world.log).toEqual([
      `merge old-1 -> ${THIS_MACHINE}`,
      "add acme/WEB",
      // The daemon was not signed in to beta: it is, before the runner is added.
      "sign_in beta",
      "add beta/API",
    ]);
    expect(outcome.restored).toEqual(["acme/WEB", "beta/API"]);
    expect(outcome.failed).toEqual([]);
    expect(outcome.ownAgentRunners).toBe(1);
  });

  it("does not add an engine runner the daemon already has", async () => {
    const world = fakeWorld({
      candidates: [candidate("old-1")],
      moved: ["r-web"],
      serverRunners: { acme: [serverRunner("r-web", "WEB", "desktop_bundled")] },
      local: [
        {
          name: "builtin-WEB",
          runner_id: "r-web",
          workspace_slug: "acme",
          project_slug: "WEB",
          working_dir: "/home/u/pidash/acme/WEB",
          agent: "engine",
        },
      ],
    });
    const prompt = await findReplacePrompt("u-1", world.deps);
    const outcome = await replaceMachine(prompt!, world.deps);
    expect(world.log.filter((l) => l.startsWith("add"))).toEqual([]);
    expect(outcome.restored).toEqual(["acme/WEB"]);
  });

  it("refuses when the daemon signed in as another machine meanwhile", async () => {
    const world = fakeWorld({ candidates: [candidate("old-1")] });
    const prompt = await findReplacePrompt("u-1", world.deps);
    world.setDiscovery(compatible("machine-other"));
    await expect(replaceMachine(prompt!, world.deps)).rejects.toThrow(/changed/);
    expect(world.log).toEqual([]);
  });

  it("reports an engine runner the daemon could not re-create, and keeps going", async () => {
    const world = fakeWorld({
      candidates: [candidate("old-1")],
      moved: ["r-web", "r-api"],
      serverRunners: {
        acme: [serverRunner("r-web", "WEB", "desktop_bundled"), serverRunner("r-api", "API", "desktop_bundled")],
      },
    });
    const runtime = world.deps.runtime as unknown as {
      addEngineRunner: (input: { workspace: string; project: string }) => Promise<unknown>;
    };
    const original = runtime.addEngineRunner.bind(runtime);
    runtime.addEngineRunner = async (input: { workspace: string; project: string }) => {
      if (input.project === "WEB") throw new Error("engine unavailable");
      return original(input);
    };
    const outcome = await replaceMachine((await findReplacePrompt("u-1", world.deps))!, world.deps);
    expect(outcome.failed).toEqual([{ project: "acme/WEB", error: "engine unavailable" }]);
    expect(outcome.restored).toEqual(["acme/API"]);
  });

  it("counts a runner the daemon's own reconcile re-added meanwhile as restored", async () => {
    const world = fakeWorld({
      candidates: [candidate("old-1")],
      moved: ["r-web"],
      serverRunners: { acme: [serverRunner("r-web", "WEB", "desktop_bundled")] },
    });
    const runtime = world.deps.runtime as unknown as {
      addEngineRunner: (input: { workspace: string; project: string }) => Promise<unknown>;
    };
    runtime.addEngineRunner = async (input: { workspace: string; project: string }) => {
      world.local.push({
        name: "builtin-WEB",
        runner_id: "r-web",
        workspace_slug: input.workspace,
        project_slug: input.project,
        working_dir: "/home/u/pidash/acme/WEB",
        agent: "engine",
      });
      throw new Error("the built-in engine is already on for WEB in acme");
    };
    const outcome = await replaceMachine((await findReplacePrompt("u-1", world.deps))!, world.deps);
    expect(outcome.restored).toEqual(["acme/WEB"]);
    expect(outcome.failed).toEqual([]);
  });

  it("offerReplace shows the prompt only when there is a machine to ask about", async () => {
    const shown: unknown[] = [];
    registerReplacePrompt((p) => shown.push(p));
    await expect(offerReplace("u-1", fakeWorld().deps)).resolves.toBe(false);
    await expect(offerReplace("u-1", fakeWorld({ candidates: [candidate("old-1")] }).deps)).resolves.toBe(true);
    expect(shown).toEqual([{ account: "u-1", prompt: { machineId: THIS_MACHINE, candidate: candidate("old-1") } }]);
    registerReplacePrompt(null);
  });

  it("turns the merge endpoint's error codes into plain text", () => {
    const apiError = (code: string) =>
      Object.assign(new Error("Request failed with status code 409"), { body: { error: code } });
    expect(errorText(apiError("source_machine_has_live_session"))).toMatch(/still connected/);
    expect(errorText(apiError("already_merged_into_another_machine"))).toMatch(/already replaced/);
    expect(errorText(apiError("target_machine_not_live"))).toMatch(/not connected right now/);
    expect(errorText(apiError("something_new"))).toBe("something_new");
    expect(errorText(new Error("disk busy"))).toBe("disk busy");
    expect(errorText({ detail: "Not allowed" })).toBe("Not allowed");
  });
});
