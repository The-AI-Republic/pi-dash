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
const invoke = vi.fn(async (command: string) =>
  command === "managed_doctor" ? { runner_present: true, engine_present: true, host_label: "desktop-test" } : {}
);
let available = true;
let issueExecutor: string | null = "managed_runner";
let projectExecutor = "managed_runner";
let accessExpired = false;
let refreshFailure: number | "network" | undefined;
let deleteFailure: number | "network" | undefined;

beforeEach(async () => {
  vi.resetModules();
  calls.length = 0;
  invoke.mockClear();
  available = true;
  issueExecutor = "managed_runner";
  projectExecutor = "managed_runner";
  accessExpired = false;
  refreshFailure = undefined;
  deleteFailure = undefined;
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
    if (method === "DELETE" && deleteFailure) fail(deleteFailure);
    if (path.endsWith("get-csrf-token/")) data = { csrf_token: "csrf-test" };
    else if (path.endsWith("agent-profile/"))
      data = {
        available,
        model: "test-model",
        base_url: "https://gateway.test/v1",
        reason_code: "byok_not_supported_on_desktop",
      };
    else if (path.endsWith("agent-token/"))
      data = {
        token: "short-lived-test-token",
        expires_at: new Date(Date.now() + 600_000).toISOString(),
      };
    else if (path.endsWith("desktop-enroll/"))
      data = { machine_token: "machine-test-token", dev_machine_id: "server-machine-id" };
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

describe("desktop agent lifecycle", () => {
  it("reuses enrollment across page reloads instead of revoking the running daemon token", async () => {
    const first = await import("../../core/services/agent-runtime");
    first.resumeAgentRuntime("user-a");
    await first.connectAgentProject("workspace-a", "project-1");
    vi.resetModules();
    const reloaded = await import("../../core/services/agent-runtime");
    reloaded.resumeAgentRuntime("user-a");
    await reloaded.connectAgentProject("workspace-a", "project-1");
    expect(calls.filter((call) => call.path.endsWith("desktop-enroll/") && call.method === "POST")).toHaveLength(1);
    await reloaded.disposeAgentRuntime();
    expect(sessionStorage.length).toBe(0);
  });

  it("does not reuse a different users enrollment", async () => {
    const first = await import("../../core/services/agent-runtime");
    first.resumeAgentRuntime("user-a");
    await first.connectAgentProject("workspace-a", "project-1");
    vi.resetModules();
    const reloaded = await import("../../core/services/agent-runtime");
    reloaded.resumeAgentRuntime("user-b");
    await reloaded.connectAgentProject("workspace-a", "project-1");
    expect(calls.filter((call) => call.path.endsWith("desktop-enroll/") && call.method === "POST")).toHaveLength(2);
  });

  it("enrolls once and preserves the selected executor without patching the issue", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    await Promise.all([
      runtime.connectAgentProject("workspace-a", "project-1"),
      runtime.prepareAgentRun({
        workspaceSlug: "workspace-a",
        projectId: "project-1",
        issueId: "issue-1",
      }),
    ]);
    expect(calls.filter((call) => call.path.endsWith("desktop-enroll/") && call.method === "POST")).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith(
      "managed_bootstrap",
      expect.objectContaining({ workspace: "workspace-a", devMachineId: "server-machine-id" })
    );
    expect(invoke).toHaveBeenCalledWith("managed_enroll", {
      workspace: "workspace-a",
      project: "TEST",
      hostLabel: "desktop-test",
    });
    expect(calls.filter((call) => call.method === "PATCH")).toEqual([]);
    expect(calls.filter((call) => call.path.endsWith("agent-token/"))).toHaveLength(1);
  });

  it("bootstraps separate workspaces and clears them on sign-out", async () => {
    const runtime = await import("../../core/services/agent-runtime");
    await runtime.connectAgentProject("workspace-a", "project-1");
    await runtime.connectAgentProject("workspace-b", "project-2");
    expect(invoke).toHaveBeenCalledWith("managed_start_daemon", { workspace: "workspace-a" });
    expect(invoke).toHaveBeenCalledWith("managed_start_daemon", { workspace: "workspace-b" });
    await runtime.disposeAgentRuntime();
    expect(calls.some((call) => call.method === "DELETE" && call.body.host_label === "desktop-test")).toBe(true);
    const starts = invoke.mock.calls.filter(([command]) => command === "managed_start_daemon").length;
    await runtime.refreshAgentRuntime();
    expect(invoke.mock.calls.filter(([command]) => command === "managed_start_daemon")).toHaveLength(starts);
    await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow("signed out");
  });

  it("explains BYOK without enrolling or pinning the issue", async () => {
    available = false;
    const runtime = await import("../../core/services/agent-runtime");
    await expect(
      runtime.prepareAgentRun({
        workspaceSlug: "workspace-a",
        projectId: "project-1",
        issueId: "issue-1",
      })
    ).rejects.toThrow(AGENT_RUNTIME_REASON_MESSAGES.byok_not_supported_on_desktop);
    expect(calls.some((call) => call.method === "PATCH" || call.path.endsWith("desktop-enroll/"))).toBe(false);
    expect(invoke).toHaveBeenCalledWith("managed_stop_daemon", { graceSeconds: 5 });
  });

  it.each([401, 403, 503, "network"])("allows logout when enrollment revocation fails with %s", async (failure) => {
    const runtime = await import("../../core/services/agent-runtime");
    runtime.resumeAgentRuntime("user-a");
    await runtime.connectAgentProject("workspace-a", "project-1");
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    deleteFailure = failure as number | "network";
    try {
      await expect(runtime.disposeAgentRuntime()).resolves.toBeUndefined();
      expect(sessionStorage.length).toBe(0);
      expect(invoke).toHaveBeenLastCalledWith("managed_sign_out", undefined);
      await expect(runtime.connectAgentProject("workspace-a", "project-1")).rejects.toThrow("signed out");
      expect(warning).toHaveBeenCalledOnce();
    } finally {
      warning.mockRestore();
    }
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
