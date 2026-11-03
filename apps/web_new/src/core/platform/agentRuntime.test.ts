// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DAEMON_STOP_GRACE_SECONDS, platform } from "./tauri.js";

type InvokeFn = (command: string, args?: unknown) => Promise<unknown>;

const NATIVE_ORIGIN = "http://localhost:8000";

function installBridge(invoke: InvokeFn): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
  win["__TAURI__"] = { core: { invoke } };
}

function removeBridge(): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  delete win["__PIDASH_NATIVE_HTTP__"];
  delete win["__TAURI__"];
}

describe("desktop agent runtime", () => {
  beforeEach(() => {
    removeBridge();
  });

  afterEach(() => {
    removeBridge();
    vi.restoreAllMocks();
  });

  it("reports available only when both bundled pieces exist", async () => {
    const invoke = vi.fn(async () => ({ runner_present: true, engine_present: true, host_label: "h" }));
    installBridge(invoke);
    await expect(platform.agentRuntime?.isAvailable()).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledWith("managed_doctor", undefined);
  });

  it("reports unavailable when a piece is missing", async () => {
    installBridge(vi.fn(async () => ({ runner_present: true, engine_present: false, host_label: "h" })));
    await expect(platform.agentRuntime?.isAvailable()).resolves.toBe(false);
  });

  it("reports unavailable when the self-check fails", async () => {
    installBridge(
      vi.fn(async () => {
        throw new Error("nope");
      })
    );
    await expect(platform.agentRuntime?.isAvailable()).resolves.toBe(false);
  });

  it("maps the host self-check with safe defaults", async () => {
    const invoke = vi.fn(async () => ({}));
    installBridge(invoke);
    await expect(platform.agentRuntime?.checkHost()).resolves.toEqual({
      runnerPresent: false,
      enginePresent: false,
      hostLabel: "",
    });
    expect(invoke).toHaveBeenCalledWith("managed_doctor", undefined);
  });

  it("writes the engine config and model token", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    const profile = { available: true, base_url: "https://models.test", model: "m", reason_code: "" };
    await platform.agentRuntime?.writeEngineConfig({ account: "u1", profile });
    await platform.agentRuntime?.writeModelToken("tok");
    expect(invoke).toHaveBeenCalledWith("managed_write_engine_config", { account: "u1", profile });
    expect(invoke).toHaveBeenCalledWith("managed_write_model_token", { token: "tok" });
  });

  it("bootstraps the machine and enrolls the project", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await platform.agentRuntime?.bootstrapMachine({
      cloudUrl: "https://cloud.test",
      workspace: "ws",
      machineToken: "mt",
      devMachineId: "dm",
    });
    await platform.agentRuntime?.enrollProject({ account: "u1", workspace: "ws", project: "P", hostLabel: "h" });
    expect(invoke).toHaveBeenCalledWith("managed_bootstrap", {
      cloudUrl: "https://cloud.test",
      workspace: "ws",
      machineToken: "mt",
      devMachineId: "dm",
    });
    expect(invoke).toHaveBeenCalledWith("managed_enroll", {
      account: "u1",
      workspace: "ws",
      project: "P",
      hostLabel: "h",
    });
  });

  it("starts the daemon and stops it with the default grace", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await platform.agentRuntime?.startDaemon({ account: "u1", workspace: "ws" });
    await platform.agentRuntime?.stopDaemon();
    await platform.agentRuntime?.stopDaemon(1);
    expect(invoke).toHaveBeenCalledWith("managed_start_daemon", { account: "u1", workspace: "ws" });
    expect(invoke).toHaveBeenCalledWith("managed_stop_daemon", { graceSeconds: DAEMON_STOP_GRACE_SECONDS });
    expect(invoke).toHaveBeenCalledWith("managed_stop_daemon", { graceSeconds: 1 });
    expect(DAEMON_STOP_GRACE_SECONDS).toBe(5);
  });

  it("signs out natively with and without history deletion", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await platform.agentRuntime?.signOutNative();
    await platform.agentRuntime?.signOutNative({ deleteChatHistoryAccount: "u1" });
    await platform.agentRuntime?.signOutNative({ deleteChatHistoryAccount: undefined });
    expect(invoke).toHaveBeenCalledWith("managed_sign_out", undefined);
    expect(invoke).toHaveBeenCalledWith("managed_sign_out", { clearChatHistory: { account: "u1" } });
  });

  it("stashes and loads approval modes", async () => {
    const invoke = vi.fn(async (command: string) =>
      command === "chat_approval_modes_load" ? { r1: "ask" } : undefined
    );
    installBridge(invoke);
    await platform.agentRuntime?.saveApprovalModes("u1", { r1: "ask" });
    await expect(platform.agentRuntime?.loadApprovalModes("u1")).resolves.toEqual({ r1: "ask" });
    expect(invoke).toHaveBeenCalledWith("chat_approval_modes_save", { account: "u1", modes: { r1: "ask" } });
    expect(invoke).toHaveBeenCalledWith("chat_approval_modes_load", { account: "u1" });
  });

  it("reads a missing mode stash as empty", async () => {
    installBridge(vi.fn(async () => null));
    await expect(platform.agentRuntime?.loadApprovalModes("u1")).resolves.toEqual({});
  });

  it("rejects runtime calls without the bridge", async () => {
    await expect(platform.agentRuntime?.checkHost()).rejects.toThrow("managed_doctor needs the native bridge");
    await expect(platform.agentRuntime?.writeModelToken("t")).rejects.toThrow(
      "managed_write_model_token needs the native bridge"
    );
    await expect(platform.agentRuntime?.isAvailable()).resolves.toBe(false);
  });
});

describe("desktop session teardown", () => {
  afterEach(() => {
    removeBridge();
    vi.restoreAllMocks();
  });

  it("wipes the webview store through the bridge", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await expect(platform.sessionTeardown?.clearLocalSession()).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("desktop_clear_web_data", undefined);
  });

  it("swallows a failed wipe so sign-out still lands", async () => {
    installBridge(
      vi.fn(async () => {
        throw new Error("wipe failed");
      })
    );
    await expect(platform.sessionTeardown?.clearLocalSession()).resolves.toBeUndefined();
  });
});
