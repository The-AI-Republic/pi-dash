/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The desktop agent runtime as a client of this computer's daemon (design
 * §4.1, §4.6, §5): no private daemon, no enrolment on project open, the engine
 * turned on for a project when it is used, and an app sign-out that signs out
 * only the app.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "http://localhost:18002" }));

const calls: { path: string; method: string; body: Record<string, unknown> }[] = [];
let discovery: Record<string, unknown>;
let localRunners: Record<string, unknown>[];
let issueExecutor: string | null;
let projectExecutor: string;
const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
  if (command === "daemon_discover") return discovery;
  if (command === "daemon_runners") return localRunners;
  if (command === "daemon_engine_runner_add")
    return { runner: "builtin-host-acme-test", runner_id: "r-1", workspace: args?.workspace, project: args?.project };
  return undefined;
});

const LEGACY_COMMANDS = new Set([
  "managed_bootstrap",
  "managed_enroll",
  "managed_start_daemon",
  "managed_stop_daemon",
  "managed_write_engine_config",
  "managed_write_model_token",
  "managed_sign_out",
  "managed_doctor",
]);

function hello(signInState = "active") {
  return {
    daemon_version: "0.2.0",
    ipc_version: 6,
    cloud_url: "http://localhost:18002",
    account: { user_id: "user-a", cloud_url: "http://localhost:18002" },
    workspaces: [{ slug: "workspace-a", signed_in: true, runners: 0 }],
    machine_id: "machine-1",
    engine: "available",
    engine_detail: { state: "available", sign_in_state: signInState },
  };
}

beforeEach(async () => {
  vi.resetModules();
  calls.length = 0;
  invoke.mockClear();
  discovery = { kind: "compatible", hello: hello() };
  localRunners = [];
  issueExecutor = "managed_runner";
  projectExecutor = "managed_runner";
  sessionStorage.clear();
  Object.assign(window, { __TAURI__: { core: { invoke } } });
  const { default: axios } = await import("axios");
  axios.defaults.adapter = async (config) => {
    const path = new URL(config.url!, config.baseURL ?? "http://localhost:18002").pathname;
    const method = (config.method ?? "get").toUpperCase();
    calls.push({ path, method, body: JSON.parse(String(config.data ?? "{}")) });
    let data: unknown = {};
    if (path.endsWith("get-csrf-token/")) data = { csrf_token: "csrf-test" };
    else if (path.endsWith("agent-profile/")) data = { top_up_url: "https://wallet.test/top-up" };
    else if (path.endsWith("device/start/")) data = { device_code: "dc-1", user_code: "UC-1" };
    else if (path.endsWith("projects/project-1/"))
      data = {
        identifier: "TEST",
        default_agent_executor: projectExecutor,
        agent_executor_options: [{ kind: "managed_runner", available: true }],
      };
    else if (path.endsWith("issues/issue-1/")) data = { agent_executor: issueExecutor };
    return { data, status: 200, statusText: "OK", headers: {}, config };
  };
});

const commands = () => invoke.mock.calls.map(([command]) => command);
const target = { workspaceSlug: "workspace-a", projectId: "project-1", issueId: "issue-1" };

describe("desktop agent runtime", () => {
  it("never runs a private daemon, writes engine files, or enrols a machine", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.refreshAgentRuntime();
    await runtime.ensureChatRuntime("workspace-a");
    await runtime.prepareAgentRun(target);
    expect(commands().filter((c) => LEGACY_COMMANDS.has(c))).toEqual([]);
    expect(calls.some((c) => c.path.includes("desktop-enroll") || c.path.includes("agent-token"))).toBe(false);
  });

  it("turns the engine on for the project at the first Run on this computer, and only once", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.prepareAgentRun(target);
    expect(invoke).toHaveBeenCalledWith("daemon_engine_runner_add", {
      workspace: "workspace-a",
      project: "TEST",
      workingDir: null,
    });
    localRunners = [
      { name: "builtin", runner_id: "r-1", workspace_slug: "workspace-a", project_slug: "TEST", agent: "engine" },
    ];
    invoke.mockClear();
    await runtime.prepareAgentRun(target);
    expect(commands()).not.toContain("daemon_engine_runner_add");
    // A Run never changes the issue's executor.
    expect(calls.some((c) => c.method === "PATCH")).toBe(false);
  });

  it.each(["local_runner", "cloud_agent"])("leaves a %s issue alone", async (executor) => {
    issueExecutor = executor;
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.prepareAgentRun(target);
    expect(commands()).not.toContain("daemon_engine_runner_add");
  });

  it("does nothing from a browser tab", async () => {
    Reflect.deleteProperty(window, "__TAURI__");
    const runtime = await import("../../core/services/agent-runtime");
    await runtime.prepareAgentRun(target);
    await runtime.ensureChatRuntime("workspace-a");
    expect(calls).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("offers set-up when there is no daemon, and refuses with the reason when declined", async () => {
    discovery = { kind: "no_daemon", cli: { installed: false, path: null, version: null, updatable: false } };
    const runtime = await import("../../core/services/agent-runtime");
    const setup = await import("../../core/services/pidash-cli");
    const asked: string[] = [];
    setup.registerSetUpPrompt((request, done) => {
      asked.push(request.reason);
      done(false);
    });
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.ensureChatRuntime("workspace-a")).rejects.toThrow("not set up");
    await expect(runtime.prepareAgentRun(target)).rejects.toThrow("not set up");
    expect(asked).toEqual(["chat", "engine"]);
    expect(commands()).not.toContain("daemon_engine_runner_add");
  });

  it("refuses local chat on another account's daemon, without offering a second daemon", async () => {
    discovery = {
      kind: "account_mismatch",
      daemon_account: "b@example.com",
      daemon_cloud_url: "",
      hosts_runners: true,
    };
    const runtime = await import("../../core/services/agent-runtime");
    const setup = await import("../../core/services/pidash-cli");
    const prompt = vi.fn();
    setup.registerSetUpPrompt(prompt);
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.ensureChatRuntime("workspace-a")).rejects.toThrow("signed in as b@example.com");
    expect(prompt).not.toHaveBeenCalled();
    await expect(runtime.agentAvailability()).resolves.toEqual({
      available: false,
      reason: "Pi Dash on this computer is signed in as b@example.com.",
    });
  });

  it("renews an expiring engine silently before a chat, but asks for a signed-out one", async () => {
    discovery = { kind: "compatible", hello: hello("expiring") };
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.ensureChatRuntime("workspace-a");
    expect(commands()).toContain("pidash_cli_engine_login");

    vi.resetModules();
    invoke.mockClear();
    discovery = { kind: "compatible", hello: hello("signed_out") };
    const again = await import("../../core/services/agent-runtime");
    again.resumeAgentRuntime("user-a");
    await expect(again.ensureChatRuntime("workspace-a")).rejects.toThrow("signed out");
    expect(commands()).not.toContain("pidash_cli_engine_login");
  });

  it("remembers where the server says model credit is added", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.refreshAgentRuntime();
    await vi.waitFor(() => expect(runtime.getModelTopUpUrl()).toBe("https://wallet.test/top-up"));
  });
});

describe("app sign-out", () => {
  it("signs out only the app by default: the daemon and its runners stay signed in", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.disposeAgentRuntime({ deleteChatHistory: false });
    expect(commands()).not.toContain("pidash_cli_logout");
    expect(commands()).not.toContain("chat_clear_history");
    // The account is still known to the session teardown that follows.
    expect(runtime.getAgentAccount()).toBe("user-a");
  });

  it("signs out Pi Dash on this computer and deletes history only when ticked", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.disposeAgentRuntime({ deleteChatHistory: true, signOutComputer: true });
    expect(invoke).toHaveBeenCalledWith("chat_clear_history", { account: "user-a" });
    expect(invoke).toHaveBeenCalledWith("pidash_cli_logout", { engine: false });
  });

  it("asks through the mounted dialog, and signs out only the app when there is none", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    await expect(runtime.confirmSignOut()).resolves.toEqual({ deleteChatHistory: false });
    runtime.registerSignOutPrompt((resolve) => resolve({ deleteChatHistory: true, signOutComputer: true }));
    await expect(runtime.confirmSignOut()).resolves.toEqual({ deleteChatHistory: true, signOutComputer: true });
  });

  it("offers to sign out the computer only for a daemon of this account", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.refreshAgentRuntime();
    expect(runtime.canSignOutComputer()).toBe(true);

    vi.resetModules();
    discovery = {
      kind: "account_mismatch",
      daemon_account: "b@example.com",
      daemon_cloud_url: "",
      hosts_runners: false,
    };
    const other = await import("../../core/services/agent-runtime");
    other.resumeAgentRuntime("user-a");
    await other.refreshAgentRuntime();
    expect(other.canSignOutComputer()).toBe(false);
  });
});

describe("reconnect after an outage", () => {
  it("wakes the daemon on refresh (window focus and the minute tick)", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.refreshAgentRuntime();
    expect(commands()).toContain("daemon_reconnect");
  });

  it("rate-limits the wake so focus storms do not hammer the daemon", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.refreshAgentRuntime();
    await runtime.refreshAgentRuntime();
    expect(commands().filter((c) => c === "daemon_reconnect")).toHaveLength(1);
  });

  it("never breaks a refresh when the daemon cannot be woken", async () => {
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "daemon_reconnect") throw new Error("daemon down");
      if (command === "daemon_discover") return discovery;
      if (command === "daemon_runners") return localRunners;
      return undefined;
    });
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await expect(runtime.refreshAgentRuntime()).resolves.toBeUndefined();
    expect(commands()).toContain("daemon_reconnect");
  });

  it("kicks the daemon before the Run availability poll, and leaves other executors alone", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.prepareAgentRun(target);
    const invoked = commands();
    expect(invoked).toContain("daemon_reconnect");
    expect(invoked.indexOf("daemon_reconnect")).toBeLessThan(invoked.indexOf("daemon_engine_runner_add"));

    invoke.mockClear();
    issueExecutor = "local_runner";
    await runtime.prepareAgentRun(target);
    expect(commands()).not.toContain("daemon_reconnect");
  });
});
