/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { AGENT_RUNTIME_REASON_MESSAGES } from "@/pi-dash-web/components/desktop/agent-runtime-edition";

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "http://localhost:18002" }));
// Exercise the public service export and the SPA's real axios setup without replacing their modules.
// Session-refresh behaviour depends on the edition's interceptor and is tested by the edition.

const calls: { path: string; method: string; body: Record<string, unknown> }[] = [];
// The CLI on this computer, as `pidash auth status --json` reports it. Signed
// in as whichever account the runtime last handed a command, unless a test
// says otherwise.
type CliState = { signedIn: boolean; user?: string; email?: string; workspace?: string | null; activeRuns?: number };
let cli: CliState = { signedIn: true };
let lastAccount = "";
let cliError: Record<string, { code: string; message: string } | undefined> = {};
const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
  if (typeof args?.account === "string") lastAccount = args.account;
  const failure = cliError[command];
  if (failure) throw failure;
  switch (command) {
    case "managed_doctor":
      return { runner_present: true, engine_present: true, host_label: "desktop-test" };
    case "pidash_auth_status":
      return {
        signed_in: cli.signedIn,
        cloud_url: "http://localhost:18002",
        user: cli.signedIn ? { id: cli.user ?? lastAccount, email: cli.email ?? "" } : null,
        workspace_slug: cli.signedIn ? (cli.workspace ?? null) : null,
        dev_machine_id: null,
        changed_at: null,
        daemon_running: true,
        active_runs: cli.activeRuns ?? 0,
      };
    case "pidash_cli_login":
      cli = { ...cli, signedIn: true, user: undefined };
      return undefined;
    case "pidash_auth_logout":
      cli = { signedIn: false };
      return undefined;
    case "managed_enroll":
      return `{"ok":true,"name":"desktop-test-${String(args?.project)}"}`;
    case "managed_start_daemon":
    case "daemon_ensure":
      return { mode: "service", cli_outdated: false, message: null };
    case "desktop_auth_observe":
      return { user_id: args?.userId ?? null, changed_at: "2026-10-26T09:00:00Z" };
    default:
      return {};
  }
});
let available = true;
let issueExecutor: string | null = "managed_runner";
let projectExecutor = "managed_runner";
let accessExpired = false;
let refreshFailure: number | "network" | undefined;
type ListedProject = { identifier: string; created_at: string; is_default?: boolean; archived_at?: string | null };
// The workspace project list, in the order the server happens to return it.
let projectList: ListedProject[] = [];
const ALPHA = { identifier: "ALPHA", created_at: "2026-01-01T00:00:00Z" };
const BETA = { identifier: "BETA", created_at: "2026-02-01T00:00:00Z" };
const GAMMA = { identifier: "GAMMA", created_at: "2026-03-01T00:00:00Z" };
const TEST = { identifier: "TEST", created_at: "2026-01-01T00:00:00Z" };
const enrolledProjects = () =>
  (invoke.mock.calls as unknown as [string, { project?: string }][])
    .filter(([command]) => command === "managed_enroll")
    .map(([, args]) => args.project);
let topUpUrl: string | undefined;

beforeEach(async () => {
  vi.resetModules();
  calls.length = 0;
  invoke.mockClear();
  available = true;
  issueExecutor = "managed_runner";
  projectExecutor = "managed_runner";
  accessExpired = false;
  refreshFailure = undefined;
  projectList = [TEST];
  topUpUrl = undefined;
  cli = { signedIn: true };
  lastAccount = "";
  cliError = {};
  sessionStorage.clear();
  Object.assign(window, { __TAURI__: { core: { invoke } } });
  const { default: axios, AxiosError } = await import("axios");
  axios.defaults.adapter = async (config) => {
    const path = new URL(config.url!, config.baseURL ?? "http://localhost:18002").pathname;
    const method = (config.method ?? "get").toUpperCase();
    calls.push({
      path,
      method,
      body: JSON.parse(String(config.data ?? "{}")),
    });
    const fail = (status: number | "network") => {
      if (status === "network") throw new AxiosError("Network error", "ERR_NETWORK", config);
      throw new AxiosError("Request failed", "ERR_BAD_REQUEST", config, undefined, {
        status,
        statusText: "error",
        config,
        headers: {},
        data: { error: "desktop_session_required" },
      });
    };
    let data: unknown = {};
    if (path.endsWith("/auth/refresh/")) {
      if (refreshFailure) fail(refreshFailure);
      accessExpired = false;
    } else if (accessExpired) fail(401);
    if (path.endsWith("get-csrf-token/")) data = { csrf_token: "csrf-test" };
    else if (path.endsWith("/auth/device/start/")) data = { device_code: "device-code-1", user_code: "USER1234" };
    else if (path.endsWith("agent-profile/"))
      data = {
        available,
        model: "test-model",
        base_url: "https://gateway.test/v1",
        reason_code: "byok_not_supported_on_desktop",
        top_up_url: topUpUrl,
      };
    else if (path.endsWith("agent-token/"))
      data = {
        token: "short-lived-test-token",
        expires_at: new Date(Date.now() + 600_000).toISOString(),
      };
    else if (/workspaces\/[^/]+\/projects\/$/.test(path)) data = projectList;
    else if (/projects\/project-\d\/$/.test(path))
      data = {
        identifier: "TEST",
        default_agent_executor: projectExecutor,
        agent_executor_options: [{ kind: "managed_runner", available: true }],
      };
    else if (path.endsWith("issues/issue-1/")) data = { agent_executor: issueExecutor };
    return { data, status: 200, statusText: "OK", headers: {}, config };
  };
});

const invoked = (command: string) =>
  (invoke.mock.calls as unknown as [string, Record<string, unknown> | undefined][]).filter(
    ([name]) => name === command
  );

describe("desktop agent lifecycle", () => {
  it("registers the engine with the shared CLI, never a per-workspace desktop enrollment", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    expect(calls.some((call) => call.path.endsWith("desktop-enroll/"))).toBe(false);
    expect(invoked("managed_bootstrap")).toEqual([]);
    expect(invoke).toHaveBeenCalledWith("managed_enroll", {
      account: "user-a",
      workspace: "workspace-a",
      project: "TEST",
      hostLabel: "desktop-test",
      chat: true,
    });
    expect(invoke).toHaveBeenCalledWith("managed_start_daemon", { account: "user-a" });
    await runtime.disposeAgentRuntime();
    expect(sessionStorage.length).toBe(0);
  });

  it("enrolls once and preserves the selected executor without patching the issue", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await Promise.all([
      runtime.connectAgentProject("workspace-a", "project-1"),
      runtime.prepareAgentRun({
        workspaceSlug: "workspace-a",
        projectId: "project-1",
        issueId: "issue-1",
      }),
    ]);
    expect(invoke).toHaveBeenCalledWith(
      "managed_enroll",
      expect.objectContaining({ account: "user-a", workspace: "workspace-a", project: "TEST" })
    );
    expect(calls.filter((call) => call.method === "PATCH")).toEqual([]);
    expect(calls.filter((call) => call.path.endsWith("agent-token/"))).toHaveLength(1);
  });

  it("signs a signed-out CLI in as this account, bound to the workspace in use", async () => {
    cli = { signedIn: false };
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    // Both halves of the device page, run by the app's own session.
    expect(calls.find((call) => call.path.endsWith("/auth/device/approve/"))?.body).toEqual({ user_code: "USER1234" });
    expect(invoke).toHaveBeenCalledWith("pidash_cli_login", {
      deviceCode: "device-code-1",
      cloudUrl: "http://localhost:18002",
      workspace: "workspace-a",
    });
    const order = invoke.mock.calls.map(([command]) => command);
    expect(order.indexOf("pidash_cli_login")).toBeLessThan(order.indexOf("managed_enroll"));
  });

  it("leaves a CLI signed in as someone else alone", async () => {
    cli = { signedIn: true, user: "user-z", email: "z@example.test" };
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow("z@example.test");
    expect(invoked("pidash_cli_login")).toEqual([]);
    expect(invoked("managed_enroll")).toEqual([]);
  });

  it("explains that the shared agent runs in the CLI's workspace", async () => {
    cli = { signedIn: true, workspace: "workspace-b" };
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow('"workspace-b" workspace');
    expect(invoked("managed_enroll")).toEqual([]);
  });

  it("asks for a CLI update instead of failing obscurely on an old CLI", async () => {
    cliError.pidash_auth_status = { code: "cli_outdated", message: "too old" };
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow(
      "Update the Pi Dash CLI"
    );
    const sync = await import("../../core/services/desktop-account-sync");
    expect(sync.getDaemonNotice().cliOutdated).toBe(true);
  });

  it("enrolls each workspace's runner into the one shared daemon and clears them on sign-out", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    await runtime.connectAgentProject("workspace-b", "project-2");
    expect(invoked("managed_enroll").map(([, args]) => args?.workspace)).toEqual(["workspace-a", "workspace-b"]);
    for (const [, args] of invoked("managed_start_daemon")) expect(args).toEqual({ account: "user-a" });
    await runtime.disposeAgentRuntime();
    const starts = invoked("managed_start_daemon").length;
    await runtime.refreshAgentRuntime();
    expect(invoked("managed_start_daemon")).toHaveLength(starts);
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow("signed out");
  });

  it("signs the CLI out before the app's own teardown, and records the sign-out", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    invoke.mockClear();
    await runtime.disposeAgentRuntime();
    const order = invoke.mock.calls.map(([command]) => command);
    expect(order[0]).toBe("pidash_auth_logout");
    expect(order).toContain("managed_sign_out");
    expect(invoke).toHaveBeenLastCalledWith("desktop_auth_observe", { userId: null });
    // The daemon is the user's: sign-out never stops it.
    expect(order.some((command) => command.includes("stop"))).toBe(false);
  });

  it("stays signed in when the CLI cannot be signed out", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    cliError.pidash_auth_logout = { code: "cli_failed", message: "cloud unreachable" };
    await expect(runtime.disposeAgentRuntime()).rejects.toThrow("cloud unreachable");
    expect(invoked("managed_sign_out")).toEqual([]);
    cliError = {};
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).resolves.toBeUndefined();
  });

  it("does not sign the CLI out when following the CLI's own change", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.signOutFollowingCli(() => runtime.disposeAgentRuntime());
    expect(invoked("pidash_auth_logout")).toEqual([]);
    expect(invoked("managed_sign_out").length).toBeGreaterThan(0);
    // The next user-initiated sign-out signs the CLI out again.
    runtime.resumeAgentRuntime("user-a");
    await runtime.disposeAgentRuntime();
    expect(invoked("pidash_auth_logout")).toHaveLength(1);
  });

  it("warns about the runs the sign-out will stop", async () => {
    cli = { signedIn: true, user: "user-a", activeRuns: 3 };
    const runtime = await import("../../core/services/agent-runtime");
    const seen: { activeRuns: number }[] = [];
    runtime.registerSignOutPrompt((resolve, context) => {
      seen.push(context);
      resolve({ deleteChatHistory: false });
    });
    await runtime.confirmSignOut();
    cli = { signedIn: false };
    await runtime.confirmSignOut();
    expect(seen).toEqual([{ activeRuns: 3 }, { activeRuns: 0 }]);
  });

  it("replaces a model credential the gateway refused, though it has not expired", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    const minted = () => calls.filter((call) => call.path.endsWith("agent-token/")).length;
    const written = () => invoke.mock.calls.filter(([command]) => command === "managed_write_model_token").length;
    expect([minted(), written()]).toEqual([1, 1]);
    // The ordinary refresh keeps a credential that still has time on it.
    await runtime.refreshAgentRuntime();
    expect(minted()).toBe(1);
    await runtime.refreshModelCredential();
    expect([minted(), written()]).toEqual([2, 2]);

    accessExpired = true;
    refreshFailure = 401;
    await expect(runtime.refreshModelCredential()).rejects.toThrow();
  });
  it("remembers where the server says model credit is added", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    expect(runtime.getModelTopUpUrl()).toBe("");
    topUpUrl = "https://hub.test/account/credits";
    await runtime.refreshAgentRuntime();
    expect(runtime.getModelTopUpUrl()).toBe("https://hub.test/account/credits");
  });
  it("points every engine command at the signed-in account's own engine home", async () => {
    // The CLI follows whichever account the app hands it (account sync's job).
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    runtime.resumeAgentRuntime("user-b");
    await runtime.connectAgentProject("workspace-a", "project-1");
    for (const command of ["managed_write_engine_config", "managed_enroll", "managed_start_daemon"]) {
      const accounts = (invoke.mock.calls as unknown as [string, { account?: string }][])
        .filter(([name]) => name === command)
        .map(([, args]) => args.account);
      expect(accounts, command).toEqual(["user-a", "user-b"]);
    }
  });

  it("refuses to run the engine with no account to scope it to", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow("signed out");
    expect(invoke.mock.calls.some(([command]) => command === "managed_enroll")).toBe(false);
  });

  it("deletes chat history on sign-out only when the user asked for it", async () => {
    const signOuts = () =>
      (invoke.mock.calls as unknown as [string, unknown][])
        .filter(([command]) => command === "managed_sign_out")
        .map(([, args]) => args);
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");

    await runtime.disposeAgentRuntime({ deleteChatHistory: true });
    // Once, for the account that is signing out; the cleanup pass that follows
    // only destroys credentials again.
    expect(signOuts()).toEqual([{ clearChatHistory: { account: "user-a" } }, undefined]);

    invoke.mockClear();
    await runtime.disposeAgentRuntime({ deleteChatHistory: false });
    await runtime.disposeAgentRuntime();
    expect(signOuts()).toEqual([undefined, undefined, undefined, undefined]);
  });

  it("never deletes chat history when an expired session tears the runtime down", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    accessExpired = true;
    await expect(runtime.connectAgentProject("workspace-a", "project-2")).rejects.toThrow();
    const signOuts = (invoke.mock.calls as unknown as [string, unknown][]).filter(
      ([command]) => command === "managed_sign_out"
    );
    expect(signOuts.length).toBeGreaterThan(0);
    for (const [, args] of signOuts) expect(args).toBeUndefined();
  });

  it("asks through the mounted dialog, and keeps history when there is none to ask through", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    await expect(runtime.confirmSignOut()).resolves.toEqual({ deleteChatHistory: false });
    runtime.registerSignOutPrompt((resolve) => resolve(null));
    await expect(runtime.confirmSignOut()).resolves.toBeNull();
    runtime.registerSignOutPrompt((resolve) => resolve({ deleteChatHistory: true }));
    await expect(runtime.confirmSignOut()).resolves.toEqual({ deleteChatHistory: true });
  });

  it("explains BYOK without enrolling, pinning the issue or stopping the shared daemon", async () => {
    available = false;
    const runtime = await import("../../core/services/agent-runtime");
    await expect(
      runtime.prepareAgentRun({
        workspaceSlug: "workspace-a",
        projectId: "project-1",
        issueId: "issue-1",
      })
    ).rejects.toThrow(AGENT_RUNTIME_REASON_MESSAGES.byok_not_supported_on_desktop);
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(invoked("managed_enroll")).toEqual([]);
    expect(invoke.mock.calls.some(([command]) => String(command).includes("stop"))).toBe(false);
  });

  it("does not provision a machine from a browser tab", async () => {
    Reflect.deleteProperty(window, "__TAURI__");
    const runtime = await import("../../core/services/agent-runtime");
    await runtime.prepareAgentRun({
      workspaceSlug: "workspace-a",
      projectId: "project-1",
      issueId: "issue-1",
    });
    expect(calls).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each(["local_runner", "cloud_agent"])("respects explicit %s even if desktop is unavailable", async (executor) => {
    issueExecutor = executor;
    available = false;
    const runtime = await import("../../core/services/agent-runtime");
    await runtime.prepareAgentRun({
      workspaceSlug: "workspace-a",
      projectId: "project-1",
      issueId: "issue-1",
    });
    expect(invoke).not.toHaveBeenCalled();
    expect(calls.every((call) => call.method === "GET")).toBe(true);
  });

  it.each(["local_runner", "cloud_agent", "managed_runner"])("respects inherited %s", async (executor) => {
    issueExecutor = null;
    projectExecutor = executor;
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.prepareAgentRun({
      workspaceSlug: "workspace-a",
      projectId: "project-1",
      issueId: "issue-1",
    });
    expect(invoke.mock.calls.some(([command]) => command === "managed_start_daemon")).toBe(
      executor === "managed_runner"
    );
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});

describe("local chat enrolment project", () => {
  it("keeps the project it enrolled when a newer project appears between messages", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    projectList = [BETA, ALPHA];
    await runtime.ensureChatRuntime("workspace-a");
    // Someone creates GAMMA; the list now leads with it.
    projectList = [GAMMA, BETA, ALPHA];
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.ensureChatRuntime("workspace-a");
    expect(new Set(enrolledProjects()).size).toBe(1);
    expect(calls.filter((call) => call.path.endsWith("workspaces/workspace-a/projects/"))).toHaveLength(1);
  });

  it("keeps that project across a page reload", async () => {
    const first = await import("../../core/services/agent-runtime");
    first.resumeAgentRuntime("user-a");
    projectList = [BETA, ALPHA];
    await first.ensureChatRuntime("workspace-a");
    vi.resetModules();
    const reloaded = await import("../../core/services/agent-runtime");
    reloaded.resumeAgentRuntime("user-a");
    projectList = [GAMMA, BETA, ALPHA];
    await reloaded.ensureChatRuntime("workspace-a");
    expect(new Set(enrolledProjects()).size).toBe(1);
  });

  it.each([
    ["the oldest project, whatever order the list arrives in", [GAMMA, ALPHA, BETA], "ALPHA"],
    ["the workspace default project over an older one", [ALPHA, { ...BETA, is_default: true }, GAMMA], "BETA"],
    [
      "an active project over an archived one",
      [{ ...ALPHA, archived_at: "2026-04-01T00:00:00Z" }, GAMMA, BETA],
      "BETA",
    ],
  ] as [string, ListedProject[], string][])("enrols %s", async (_name, listed, expected) => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    projectList = listed;
    await runtime.ensureChatRuntime("workspace-a");
    expect(enrolledProjects()).toEqual([expected]);
  });

  it("reuses the project the user already connected instead of enrolling another", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    projectList = [GAMMA, BETA, ALPHA];
    await runtime.ensureChatRuntime("workspace-a");
    expect(new Set(enrolledProjects())).toEqual(new Set(["TEST"]));
  });

  it("re-resolves the project after sign-out", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    projectList = [ALPHA];
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.disposeAgentRuntime();
    runtime.resumeAgentRuntime("user-a");
    projectList = [BETA];
    await runtime.ensureChatRuntime("workspace-a");
    expect(enrolledProjects()).toEqual(["ALPHA", "BETA"]);
  });

  it("still asks for a project when the workspace has none", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    projectList = [];
    await expect(runtime.ensureChatRuntime("workspace-a")).rejects.toThrow("Create a project");
    expect(enrolledProjects()).toEqual([]);
  });
});

describe("local chat runtime hot path", () => {
  const cloudCalls = (suffix: string) => calls.filter((call) => call.path.endsWith(suffix));

  it("provisions once, then only checks the daemon on later messages", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.ensureChatRuntime("workspace-a");
    const provisioning = calls.length;
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.ensureChatRuntime("workspace-a");
    expect(calls.slice(provisioning).map((call) => `${call.method} ${call.path}`)).toEqual([]);
    expect(cloudCalls("agent-profile/")).toHaveLength(1);
    expect(cloudCalls("workspaces/workspace-a/projects/")).toHaveLength(1);
    expect(invoked("managed_enroll")).toHaveLength(1);
    expect(invoked("managed_write_engine_config")).toHaveLength(1);
    // The daemon check is the one step kept: it restarts a daemon that died.
    expect(invoked("managed_start_daemon")).toHaveLength(3);
    for (const [, args] of invoked("managed_start_daemon")) {
      expect(args).toEqual({ account: "user-a" });
    }
  });

  it("coalesces a burst of messages into one provisioning pass", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await Promise.all([
      runtime.ensureChatRuntime("workspace-a"),
      runtime.ensureChatRuntime("workspace-a"),
      runtime.ensureChatRuntime("workspace-a"),
    ]);
    expect(cloudCalls("agent-profile/")).toHaveLength(1);
    expect(invoked("managed_enroll")).toHaveLength(1);
  });

  it("provisions each workspace separately", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.ensureChatRuntime("workspace-b");
    expect(invoked("managed_enroll").map(([, args]) => args?.workspace)).toEqual(["workspace-a", "workspace-b"]);
  });

  it("renews an expiring model credential without enrolling again", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      const runtime = await import("../../core/services/agent-runtime");
      runtime.resumeAgentRuntime("user-a");
      await runtime.ensureChatRuntime("workspace-a");
      // The mocked credential lasts 10 minutes; step to within its renewal window.
      vi.setSystemTime(Date.now() + 540_000);
      await runtime.ensureChatRuntime("workspace-a");
      expect(cloudCalls("agent-token/")).toHaveLength(2);
      expect(invoked("managed_enroll")).toHaveLength(1);
      expect(cloudCalls("workspaces/workspace-a/projects/")).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("provisions again after sign-out and a new sign-in", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.disposeAgentRuntime();
    await expect(runtime.ensureChatRuntime("workspace-a")).rejects.toThrow("signed out");
    runtime.resumeAgentRuntime("user-b");
    await runtime.ensureChatRuntime("workspace-a");
    expect(invoked("managed_enroll")).toHaveLength(2);
  });

  it("does not restart the daemon for a message once the agent became unavailable", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.ensureChatRuntime("workspace-a");
    available = false;
    await expect(runtime.refreshAgentRuntime()).rejects.toThrow(
      AGENT_RUNTIME_REASON_MESSAGES.byok_not_supported_on_desktop
    );
    const starts = invoked("managed_start_daemon").length;
    await expect(runtime.ensureChatRuntime("workspace-a")).rejects.toThrow(
      AGENT_RUNTIME_REASON_MESSAGES.byok_not_supported_on_desktop
    );
    expect(invoked("managed_start_daemon")).toHaveLength(starts);
  });
});
