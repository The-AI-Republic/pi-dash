// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    startDeviceGrant: vi.fn(),
    approveDeviceCode: vi.fn(),
    getProject: vi.fn(),
    getIssue: vi.fn(),
  };
});

import {
  approveDeviceCode,
  getIssue,
  getProject,
  startDeviceGrant,
  type ApiClient,
  type DeviceStartResponse,
  type Issue,
  type ProjectDetail,
} from "@pidash/api-client";

import type {
  AgentRuntimeApi,
  CliSetupApi,
  DaemonDiscovery,
  DaemonEngineSignInState,
  DaemonHello,
  DaemonLocalRunner,
} from "../../../core/platform/index.js";
import {
  canSignOutComputer,
  compatibleHello,
  daemonAccountKey,
  discoverDaemon,
  discoveryProblem,
  disposeRuntimeForSignOut,
  ENGINE_SIGNED_OUT_TEXT,
  ENGINE_STILL_CONNECTING_TEXT,
  ENGINE_TURN_ON_TEXT,
  EngineUnavailableError,
  engineActionFor,
  ensureChatRuntime,
  getRuntimeAccount,
  prepareAgentRun,
  refreshRuntime,
  renewEngineIfAllowed,
  resetRuntimeServiceForTests,
  resumeRuntime,
  setUpDaemon,
  signInWorkspace,
  SILENT_RENEW_RETRY_MS,
  turnOnEngineForProject,
  type DaemonSetUpStep,
} from "./service.js";
import { registerSetUpPrompt } from "./setupPrompt.js";

const CLOUD = "https://cloud.test";
const client = { baseUrl: CLOUD } as unknown as ApiClient;
const CLI_INFO = { installed: true, path: "/bin/pidash", version: "1.0.0", updatable: false };

function hello(signIn: DaemonEngineSignInState = "active", overrides: Partial<DaemonHello> = {}): DaemonHello {
  return {
    daemon_version: "1.0.0",
    ipc_version: 3,
    cloud_url: CLOUD,
    account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
    workspaces: [{ slug: "ws", signed_in: true, runners: 1 }],
    engine: "available",
    engine_detail: { state: "available", sign_in_state: signIn },
    ...overrides,
  };
}

function compatible(signIn: DaemonEngineSignInState = "active", overrides: Partial<DaemonHello> = {}): DaemonDiscovery {
  return { kind: "compatible", hello: hello(signIn, overrides) };
}

const NO_DAEMON: DaemonDiscovery = { kind: "no_daemon", cli: { ...CLI_INFO, installed: false } };

type FakeRuntime = { [K in keyof AgentRuntimeApi]: ReturnType<typeof vi.fn> & AgentRuntimeApi[K] };

function fakeRuntime(discovery: DaemonDiscovery = compatible()): FakeRuntime {
  return {
    discover: vi.fn(async () => discovery),
    localRunners: vi.fn(async (): Promise<DaemonLocalRunner[]> => []),
    addEngineRunner: vi.fn(async (input: { workspace: string; project: string }) => ({
      runner: `engine-${input.project}`,
      runner_id: "r-new",
      workspace: input.workspace,
      project: input.project,
      working_dir: "/tasks",
    })),
    installService: vi.fn(async () => undefined),
    ensureEngine: vi.fn(async () => undefined),
    engineLogin: vi.fn(async () => undefined),
    reloadCredentials: vi.fn(async () => undefined),
    saveApprovalModes: vi.fn(async () => undefined),
    loadApprovalModes: vi.fn(async () => ({})),
    scanLegacyTrees: vi.fn(async () => []),
    finishLegacyTree: vi.fn(async () => undefined),
    restoreLegacyFolder: vi.fn(async () => undefined),
    loadNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    recordNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    dismissNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    signOutComputer: vi.fn(async () => undefined),
  } as unknown as FakeRuntime;
}

function fakeCli(installed = true): CliSetupApi & { [K in keyof CliSetupApi]: ReturnType<typeof vi.fn> } {
  return {
    detect: vi.fn(async () => ({ installed, path: installed ? "/bin/pidash" : null, version: null })),
    install: vi.fn(async () => ({ installed: true, path: "/bin/pidash", version: "1.0.0" })),
    onInstallLog: vi.fn(() => () => undefined),
    login: vi.fn(async () => undefined),
  } as unknown as CliSetupApi & { [K in keyof CliSetupApi]: ReturnType<typeof vi.fn> };
}

function grant(code: string): DeviceStartResponse {
  return {
    device_code: `device-${code}`,
    user_code: `USER-${code}`,
    verification_uri: "",
    expires_in: 600,
    interval: 5,
  };
}

function resume(runtime: AgentRuntimeApi, id = "u1"): void {
  resumeRuntime({ id, email: `${id}@test`, cloudUrl: CLOUD }, { client, runtime });
}

function project(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
  return {
    identifier: "PROJ",
    default_agent_executor: "local_runner",
    agent_executor_options: [],
    ...overrides,
  } as unknown as ProjectDetail;
}

function issue(executor: string | null): Issue {
  return { agent_executor: executor } as unknown as Issue;
}

beforeEach(() => {
  resetRuntimeServiceForTests();
  registerSetUpPrompt(null);
  vi.mocked(startDeviceGrant).mockReset();
  vi.mocked(approveDeviceCode).mockReset();
  vi.mocked(getProject).mockReset();
  vi.mocked(getIssue).mockReset();
  vi.mocked(startDeviceGrant).mockResolvedValue(grant("1"));
  vi.mocked(approveDeviceCode).mockResolvedValue({ ok: true, user_email: "u1@test.dev", workspace_slug: "ws" });
});

afterEach(() => {
  registerSetUpPrompt(null);
});

describe("engineActionFor", () => {
  it("maps each engine sign-in state to what the app may do", () => {
    expect(engineActionFor("expiring")).toBe("renew_silently");
    expect(engineActionFor("expired")).toBe("renew_silently");
    expect(engineActionFor("never")).toBe("turn_on");
    expect(engineActionFor("signed_out")).toBe("sign_in");
    expect(engineActionFor("revoked")).toBe("sign_in");
    expect(engineActionFor("active")).toBe("none");
    expect(engineActionFor(undefined)).toBe("none");
  });
});

describe("discoveryProblem", () => {
  it("names every non-compatible case and nothing for compatible", () => {
    expect(discoveryProblem(null)).toContain("Looking for Pi Dash");
    expect(discoveryProblem(compatible())).toBeNull();
    expect(discoveryProblem(NO_DAEMON)).toBe("Pi Dash is not set up on this computer yet.");
    expect(discoveryProblem({ kind: "too_old", cli: CLI_INFO })).toContain("too old");
    expect(discoveryProblem({ kind: "app_too_old", daemon_version: "9.0.0", ipc_version: 9 })).toContain("9.0.0");
    expect(discoveryProblem({ kind: "not_answering", reason: "busy" })).toContain("not answering");
    expect(discoveryProblem({ kind: "signed_out", hello: hello() })).toContain("signed out");
    expect(
      discoveryProblem({
        kind: "account_mismatch",
        daemon_account: "x@y",
        daemon_cloud_url: CLOUD,
        hosts_runners: true,
      })
    ).toContain("x@y");
  });
});

describe("resume and discovery", () => {
  it("is inert without the desktop capability", async () => {
    resumeRuntime({ id: "u1", cloudUrl: CLOUD }, { client });
    expect(getRuntimeAccount()).toBe("");
    await expect(discoverDaemon(undefined)).resolves.toBeNull();
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client })).resolves.toBeUndefined();
    await expect(
      prepareAgentRun({ client, target: { workspaceSlug: "ws", projectId: "p", issueId: "i" } })
    ).resolves.toBeUndefined();
    await expect(
      disposeRuntimeForSignOut({ client, choice: { deleteChatHistory: true, signOutComputer: false } })
    ).resolves.toBeUndefined();
    expect(getProject).not.toHaveBeenCalled();
  });

  it("classifies the daemon for the resumed account", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    expect(getRuntimeAccount()).toBe("u1");
    await discoverDaemon(runtime);
    expect(runtime.discover).toHaveBeenCalledWith({ userId: "u1", email: "u1@test", cloudUrl: CLOUD });
    expect(compatibleHello()).not.toBeNull();
  });

  it("restores the account's approval modes once per account", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    resume(runtime);
    expect(runtime.loadApprovalModes).toHaveBeenCalledTimes(1);
    resume(runtime, "u2");
    expect(runtime.loadApprovalModes).toHaveBeenCalledTimes(2);
    expect(runtime.loadApprovalModes).toHaveBeenLastCalledWith("u2");
  });

  it("drops a discovery that answers after an account switch", async () => {
    let answer: (value: DaemonDiscovery) => void = () => undefined;
    const runtime = fakeRuntime();
    runtime.discover.mockImplementationOnce(() => new Promise<DaemonDiscovery>((resolve) => (answer = resolve)));
    resume(runtime);
    const stale = discoverDaemon(runtime);
    resume(runtime, "u2");
    answer(compatible());
    await expect(stale).resolves.toBeNull();
    expect(compatibleHello()).toBeNull();
  });

  it("never calls a removed managed_* surface", () => {
    const runtime = fakeRuntime();
    for (const key of Object.keys(runtime)) expect(key).not.toMatch(/managed|doctor|daemonStart|signOutNative/i);
  });
});

describe("silent engine renewal", () => {
  it("renews only when the daemon reports expiring or expired", async () => {
    for (const signIn of ["active", "never", "signed_out", "revoked"] as const) {
      resetRuntimeServiceForTests();
      const runtime = fakeRuntime(compatible(signIn));
      resume(runtime);
      await refreshRuntime({ client, runtime });
      expect(runtime.engineLogin).not.toHaveBeenCalled();
    }
    for (const signIn of ["expiring", "expired"] as const) {
      resetRuntimeServiceForTests();
      const runtime = fakeRuntime(compatible(signIn));
      resume(runtime);
      await refreshRuntime({ client, runtime });
      expect(startDeviceGrant).toHaveBeenLastCalledWith(client, { scope: "engine" });
      expect(approveDeviceCode).toHaveBeenLastCalledWith(client, "USER-1", { interactive: false });
      expect(runtime.engineLogin).toHaveBeenCalledWith("device-1");
    }
  });

  it("does not renew for a daemon that is not the app's", async () => {
    const runtime = fakeRuntime({ kind: "signed_out", hello: hello("expired") });
    resume(runtime);
    await refreshRuntime({ client, runtime });
    expect(runtime.engineLogin).not.toHaveBeenCalled();
  });

  it("waits before retrying a failed renewal and never throws", async () => {
    const runtime = fakeRuntime(compatible("expired"));
    runtime.engineLogin.mockRejectedValue(new Error("cli failed"));
    resume(runtime);
    await discoverDaemon(runtime);
    const start = Date.now();
    await expect(renewEngineIfAllowed({ client, runtime }, start)).resolves.toBe(false);
    await expect(renewEngineIfAllowed({ client, runtime }, start + 1_000)).resolves.toBe(false);
    expect(runtime.engineLogin).toHaveBeenCalledTimes(1);
    await renewEngineIfAllowed({ client, runtime }, start + SILENT_RENEW_RETRY_MS);
    expect(runtime.engineLogin).toHaveBeenCalledTimes(2);
  });

  it("keeps the last discovery when the shell call fails", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await refreshRuntime({ client, runtime });
    runtime.discover.mockRejectedValueOnce(new Error("socket gone"));
    await expect(refreshRuntime({ client, runtime })).resolves.toBeUndefined();
    expect(compatibleHello()).not.toBeNull();
  });
});

describe("ensureChatRuntime", () => {
  it("passes for a compatible daemon with an active engine, setting nothing up", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).resolves.toBeUndefined();
    expect(runtime.installService).not.toHaveBeenCalled();
    expect(runtime.addEngineRunner).not.toHaveBeenCalled();
  });

  it("refuses with the explicit sign-in the engine state needs", async () => {
    const turnOn = fakeRuntime(compatible("never"));
    resume(turnOn);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime: turnOn })).rejects.toThrow(
      ENGINE_TURN_ON_TEXT
    );
    resetRuntimeServiceForTests();
    const signedOut = fakeRuntime(compatible("signed_out"));
    resume(signedOut);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime: signedOut })).rejects.toThrow(
      ENGINE_SIGNED_OUT_TEXT
    );
    expect(signedOut.engineLogin).not.toHaveBeenCalled();
  });

  it("names the engine's unavailability reason", async () => {
    const runtime = fakeRuntime(
      compatible("active", {
        engine: "unavailable",
        engine_detail: { state: "unavailable", sign_in_state: "active", reason_code: "managed_runner_disabled" },
      })
    );
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toBeInstanceOf(
      EngineUnavailableError
    );
  });

  it("refuses a daemon signed in as someone else without offering set-up", async () => {
    const prompt = vi.fn();
    registerSetUpPrompt(prompt);
    const runtime = fakeRuntime({
      kind: "account_mismatch",
      daemon_account: "other@test",
      daemon_cloud_url: CLOUD,
      hosts_runners: false,
    });
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toThrow("other@test");
    expect(prompt).not.toHaveBeenCalled();
  });

  it("asks for set-up when there is no daemon and refuses when declined", async () => {
    const prompt = vi.fn((_request: unknown, done: (ok: boolean) => void) => done(false));
    registerSetUpPrompt(prompt);
    const runtime = fakeRuntime(NO_DAEMON);
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toThrow(
      "Pi Dash is not set up on this computer yet."
    );
    expect(prompt).toHaveBeenCalledWith({ reason: "chat", workspaceSlug: "ws" }, expect.any(Function));
    expect(runtime.installService).not.toHaveBeenCalled();
  });

  it("refuses without a consent dialog: nothing is set up silently", async () => {
    const runtime = fakeRuntime(NO_DAEMON);
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toThrow("not set up");
    expect(runtime.installService).not.toHaveBeenCalled();
  });

  it("continues once the user accepted set-up", async () => {
    const runtime = fakeRuntime(NO_DAEMON);
    const cli = fakeCli();
    registerSetUpPrompt((request, done) => {
      runtime.discover.mockResolvedValue(compatible());
      void setUpDaemon({ client, runtime, cli, workspaceSlug: request.workspaceSlug }).then(() => done(true));
    });
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).resolves.toBeUndefined();
    expect(runtime.installService).toHaveBeenCalledTimes(1);
  });
});

describe("setUpDaemon", () => {
  it("installs, signs in to the workspace, installs the service and the engine, in order", async () => {
    const runtime = fakeRuntime(NO_DAEMON);
    const cli = fakeCli(false);
    const steps: DaemonSetUpStep[] = [];
    resume(runtime);
    runtime.ensureEngine.mockImplementation(async () => {
      runtime.discover.mockResolvedValue(compatible());
    });
    await setUpDaemon({ client, runtime, cli, workspaceSlug: "ws", onStep: (step) => steps.push(step) });
    expect(steps).toEqual(["install_cli", "sign_in", "service", "engine", "done"]);
    expect(cli.install).toHaveBeenCalledTimes(1);
    expect(startDeviceGrant).toHaveBeenCalledWith(client);
    expect(approveDeviceCode).toHaveBeenCalledWith(client, "USER-1", { interactive: false, workspaceSlug: "ws" });
    expect(cli.login).toHaveBeenCalledWith({ deviceCode: "device-1", cloudUrl: CLOUD, workspace: "ws" });
    expect(runtime.installService).toHaveBeenCalledTimes(1);
    expect(runtime.ensureEngine).toHaveBeenCalledTimes(1);
    expect(compatibleHello()).not.toBeNull();
  });

  it("skips installing and signing in when the daemon is already the account's", async () => {
    const runtime = fakeRuntime(compatible());
    const cli = fakeCli(true);
    resume(runtime);
    await setUpDaemon({ client, runtime, cli, workspaceSlug: "ws" });
    expect(cli.install).not.toHaveBeenCalled();
    expect(cli.login).not.toHaveBeenCalled();
    expect(runtime.installService).toHaveBeenCalledTimes(1);
  });

  it("refuses without the command-line capability or before resume", async () => {
    const runtime = fakeRuntime(NO_DAEMON);
    await expect(setUpDaemon({ client, runtime, cli: fakeCli(), workspaceSlug: "ws" })).rejects.toThrow("signed out");
    resume(runtime);
    await expect(setUpDaemon({ client, runtime, cli: undefined, workspaceSlug: "ws" })).rejects.toThrow(
      "cannot set up"
    );
  });
});

describe("turning the engine on for a project", () => {
  it("reuses the engine runner the computer already has", async () => {
    const runtime = fakeRuntime();
    runtime.localRunners.mockResolvedValue([
      { name: "own", runner_id: "a", workspace_slug: "ws", project_slug: "PROJ", working_dir: "/a", agent: "codex" },
      { name: "eng", runner_id: "b", workspace_slug: "ws", project_slug: "PROJ", working_dir: "/b", agent: "engine" },
    ]);
    resume(runtime);
    await expect(
      turnOnEngineForProject({ client, runtime, workspaceSlug: "ws", project: "PROJ" })
    ).resolves.toMatchObject({
      runner_id: "b",
    });
    expect(runtime.addEngineRunner).not.toHaveBeenCalled();
  });

  it("adds an engine runner when the project has none", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await turnOnEngineForProject({ client, runtime, workspaceSlug: "ws", project: "PROJ" });
    expect(runtime.addEngineRunner).toHaveBeenCalledWith({ workspace: "ws", project: "PROJ", workingDir: undefined });
  });
});

describe("prepareAgentRun", () => {
  const target = { workspaceSlug: "ws", projectId: "p1", issueId: "i1" };

  it("leaves any other executor alone", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getIssue).mockResolvedValue(issue(null));
    vi.mocked(getProject).mockResolvedValue(project());
    await prepareAgentRun({ client, runtime, target });
    expect(runtime.discover).not.toHaveBeenCalled();
    expect(runtime.addEngineRunner).not.toHaveBeenCalled();
  });

  it("enrols the engine on first use and waits for the server to see it", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getIssue).mockResolvedValue(issue("managed_runner"));
    vi.mocked(getProject)
      .mockResolvedValueOnce(project())
      .mockResolvedValueOnce(
        project({ agent_executor_options: [{ kind: "managed_runner", available: false }] } as never)
      )
      .mockResolvedValueOnce(
        project({ agent_executor_options: [{ kind: "managed_runner", available: true }] } as never)
      );
    const sleep = vi.fn(async () => undefined);
    await prepareAgentRun({ client, runtime, target, heartbeatPoll: { sleep, intervalMs: 1 } });
    expect(runtime.addEngineRunner).toHaveBeenCalledWith({ workspace: "ws", project: "PROJ", workingDir: undefined });
    expect(sleep).toHaveBeenCalledTimes(1);
  });

  it("follows the project default when the issue pins nothing", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getIssue).mockResolvedValue(issue(null));
    vi.mocked(getProject).mockResolvedValue(
      project({
        default_agent_executor: "managed_runner",
        agent_executor_options: [{ kind: "managed_runner", available: true }],
      } as never)
    );
    await prepareAgentRun({ client, runtime, target });
    expect(runtime.addEngineRunner).toHaveBeenCalledTimes(1);
  });

  it("gives up after the heartbeat wait", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getIssue).mockResolvedValue(issue("managed_runner"));
    vi.mocked(getProject).mockResolvedValue(project());
    await expect(
      prepareAgentRun({ client, runtime, target, heartbeatPoll: { maxAttempts: 2, sleep: async () => undefined } })
    ).rejects.toThrow(ENGINE_STILL_CONNECTING_TEXT);
  });
});

describe("signInWorkspace and sign-out gating", () => {
  it("signs the daemon in to one more workspace with an approved grant", async () => {
    const runtime = fakeRuntime(compatible());
    const cli = fakeCli();
    resume(runtime);
    await discoverDaemon(runtime);
    await signInWorkspace({ client, runtime, cli, workspaceSlug: "ws" });
    expect(vi.mocked(startDeviceGrant)).toHaveBeenCalledWith(client);
    expect(vi.mocked(approveDeviceCode)).toHaveBeenCalledWith(client, "USER-1", {
      interactive: false,
      workspaceSlug: "ws",
    });
    expect(cli.login).toHaveBeenCalledWith({ deviceCode: "device-1", cloudUrl: CLOUD, workspace: "ws" });
    expect(runtime.reloadCredentials).toHaveBeenCalledTimes(1);
    expect(runtime.discover).toHaveBeenCalledTimes(2);
  });

  it("keys the engine home by user id, else e-mail", async () => {
    expect(daemonAccountKey(hello())).toBe("u1");
    expect(daemonAccountKey({ ...hello(), account: { cloud_url: CLOUD, email: "e@test" } })).toBe("e@test");
    expect(daemonAccountKey(null)).toBeUndefined();
  });

  it("offers computer sign-out only when the daemon is the account's", async () => {
    const runtime = fakeRuntime(compatible());
    resume(runtime);
    await discoverDaemon(runtime);
    expect(canSignOutComputer()).toBe(true);
    resetRuntimeServiceForTests();
    expect(canSignOutComputer()).toBe(false);
    resume(runtime);
    await discoverDaemon(fakeRuntime(NO_DAEMON));
    expect(canSignOutComputer()).toBe(false);
  });
});

describe("disposeRuntimeForSignOut", () => {
  it("deletes this account's chat history only when asked", async () => {
    const runtime = fakeRuntime();
    const chat = { clearHistory: vi.fn(async () => undefined) };
    resume(runtime);
    await disposeRuntimeForSignOut({
      client,
      runtime,
      chat,
      choice: { deleteChatHistory: false, signOutComputer: false },
    });
    expect(chat.clearHistory).not.toHaveBeenCalled();
    resume(runtime);
    await disposeRuntimeForSignOut({
      client,
      runtime,
      chat,
      choice: { deleteChatHistory: true, signOutComputer: false },
    });
    expect(chat.clearHistory).toHaveBeenCalledWith("u1");
  });

  it("leaves the daemon and engine signed in and forgets discovery", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await discoverDaemon(runtime);
    await disposeRuntimeForSignOut({ client, runtime, choice: { deleteChatHistory: false, signOutComputer: false } });
    expect(runtime.engineLogin).not.toHaveBeenCalled();
    expect(compatibleHello()).toBeNull();
    await expect(discoverDaemon(runtime)).resolves.toBeNull();
    // The account stays readable until the page navigates away.
    expect(getRuntimeAccount()).toBe("u1");
  });

  it("never rejects when the history delete or the mode stash fails", async () => {
    const runtime = fakeRuntime();
    runtime.saveApprovalModes.mockRejectedValue(new Error("stash failed"));
    globalThis.localStorage?.setItem("pidash:chat-approval-mode:r1", "ask");
    const chat = { clearHistory: vi.fn(async () => Promise.reject(new Error("io"))) };
    resume(runtime);
    await expect(
      disposeRuntimeForSignOut({ client, runtime, chat, choice: { deleteChatHistory: true, signOutComputer: false } })
    ).resolves.toBeUndefined();
  });

  it("signs out the computer only when ticked, and never rejects when it fails", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await disposeRuntimeForSignOut({ client, runtime, choice: { deleteChatHistory: false, signOutComputer: false } });
    expect(runtime.signOutComputer).not.toHaveBeenCalled();
    resume(runtime);
    await disposeRuntimeForSignOut({ client, runtime, choice: { deleteChatHistory: false, signOutComputer: true } });
    expect(runtime.signOutComputer).toHaveBeenCalledTimes(1);
    resume(runtime);
    runtime.signOutComputer.mockRejectedValue(new Error("logout failed"));
    await expect(
      disposeRuntimeForSignOut({ client, runtime, choice: { deleteChatHistory: false, signOutComputer: true } })
    ).resolves.toBeUndefined();
  });
});
