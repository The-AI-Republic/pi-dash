/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * "Is this <label>? Replace it" (design §4.10) against a fake host, server
 * and settings store: which machine is asked about, what confirming does
 * (merge into this machine, then re-create the moved engine runners on the
 * daemon), and that declining is remembered.
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

import type { Discovery, LocalRunner } from "@/services/desktop-daemon";
import { mergePath } from "@/services/desktop-migration";
import {
  declineReplace,
  findReplacePrompt,
  MY_WORKSPACES_PATH,
  offerReplace,
  registerReplacePrompt,
  replaceCandidatesPath,
  replaceMachine,
  RUNNERS_PATH,
  type ReplaceCandidate,
  type ReplaceDeps,
} from "@/services/desktop-replace";

const THIS_MACHINE = "machine-new";

function compatible(machineId = THIS_MACHINE, workspaces = [{ slug: "acme", signed_in: true, runners: 0 }]): Discovery {
  return {
    kind: "compatible",
    hello: {
      daemon_version: "0.2.0",
      ipc_version: 6,
      cloud_url: "https://pidash.test",
      account: { user_id: "u-1", cloud_url: "https://pidash.test" },
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

/** A fake world: server candidates and runners, the daemon, the settings file. */
function fakeWorld(
  options: {
    discovery?: Discovery | null;
    candidates?: ReplaceCandidate[];
    moved?: string[];
    serverRunners?: Record<string, { id: string; is_engine: boolean; pod_detail: { project_identifier: string } }[]>;
    local?: LocalRunner[];
  } = {}
) {
  let discovery = options.discovery === undefined ? compatible() : options.discovery;
  let dismissed: string[] = [];
  const log: string[] = [];
  const deps: ReplaceDeps = {
    discovery: () => discovery,
    get: async <T>(path: string, params?: Record<string, string>) => {
      if (path === replaceCandidatesPath(THIS_MACHINE)) return { candidates: options.candidates ?? [] } as T;
      if (path === MY_WORKSPACES_PATH) {
        return [
          { id: "ws-acme", slug: "acme" },
          { id: "ws-beta", slug: "beta" },
        ] as T;
      }
      if (path === RUNNERS_PATH) return (options.serverRunners?.[params?.workspace ?? ""] ?? []) as T;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async <T>(path: string, body: unknown) => {
      log.push(`POST ${path} ${JSON.stringify(body)}`);
      return {
        source_dev_machine_id: "old",
        target_dev_machine_id: THIS_MACHINE,
        moved_runner_ids: options.moved ?? [],
        collisions: [],
      } as T;
    },
    signInWorkspace: async (slug) => {
      log.push(`sign_in ${slug}`);
      discovery = compatible(THIS_MACHINE, [
        { slug: "acme", signed_in: true, runners: 0 },
        { slug, signed_in: true, runners: 0 },
      ]);
    },
    invoke: async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "desktop_notices_load") {
        return { notices: { legacy_folders: [], pod_member_projects: [] }, dismissed } as T;
      }
      if (command === "desktop_notices_dismiss") {
        dismissed = [...dismissed, ...((args?.keys as string[] | undefined) ?? [])];
        return { notices: { legacy_folders: [], pod_member_projects: [] }, dismissed } as T;
      }
      if (command === "daemon_runners") return (options.local ?? []) as T;
      log.push(`${command} ${JSON.stringify(args ?? {})}`);
      return { runner: "x", runner_id: "x" } as T;
    },
  };
  return { deps, log, dismissed: () => dismissed, setDiscovery: (d: Discovery | null) => (discovery = d) };
}

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
        "ws-acme": [
          { id: "r-web", is_engine: true, pod_detail: { project_identifier: "WEB" } },
          { id: "r-own", is_engine: false, pod_detail: { project_identifier: "WEB" } },
          { id: "unrelated", is_engine: true, pod_detail: { project_identifier: "OPS" } },
        ],
        "ws-beta": [{ id: "r-beta", is_engine: true, pod_detail: { project_identifier: "API" } }],
      },
    });
    const prompt = await findReplacePrompt("u-1", world.deps);
    const outcome = await replaceMachine(prompt!, world.deps);
    expect(world.log).toEqual([
      `POST ${mergePath("old-1")} {"target":"${THIS_MACHINE}"}`,
      'daemon_engine_runner_add {"workspace":"acme","project":"WEB"}',
      // The daemon was not signed in to beta: it is, before the runner is added.
      "sign_in beta",
      'daemon_engine_runner_add {"workspace":"beta","project":"API"}',
    ]);
    expect(outcome.restored).toEqual(["acme/WEB", "beta/API"]);
    expect(outcome.failed).toEqual([]);
    expect(outcome.ownAgentRunners).toBe(1);
  });

  it("does not add an engine runner the daemon already has", async () => {
    const world = fakeWorld({
      candidates: [candidate("old-1")],
      moved: ["r-web"],
      serverRunners: { "ws-acme": [{ id: "r-web", is_engine: true, pod_detail: { project_identifier: "WEB" } }] },
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
    expect(world.log.filter((l) => l.startsWith("daemon_engine_runner_add"))).toEqual([]);
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
        "ws-acme": [
          { id: "r-web", is_engine: true, pod_detail: { project_identifier: "WEB" } },
          { id: "r-api", is_engine: true, pod_detail: { project_identifier: "API" } },
        ],
      },
    });
    const invoke = world.deps.invoke;
    world.deps.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "daemon_engine_runner_add" && args?.project === "WEB") throw new Error("engine unavailable");
      return invoke<T>(command, args);
    };
    const outcome = await replaceMachine((await findReplacePrompt("u-1", world.deps))!, world.deps);
    expect(outcome.failed).toEqual([{ project: "acme/WEB", error: "engine unavailable" }]);
    expect(outcome.restored).toEqual(["acme/API"]);
  });

  it("offerReplace shows the prompt only when there is a machine to ask about", async () => {
    const shown: unknown[] = [];
    registerReplacePrompt((p) => shown.push(p));
    await expect(offerReplace("u-1", fakeWorld().deps)).resolves.toBe(false);
    await expect(offerReplace("u-1", fakeWorld({ candidates: [candidate("old-1")] }).deps)).resolves.toBe(true);
    expect(shown).toEqual([{ account: "u-1", prompt: { machineId: THIS_MACHINE, candidate: candidate("old-1") } }]);
    registerReplacePrompt(null);
  });
});
