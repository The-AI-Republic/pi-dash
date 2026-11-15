// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { platform } from "./tauri.js";

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

  it("discovers the daemon for the app's account", async () => {
    const found = { kind: "no_daemon", cli: { installed: false, path: null, version: null, updatable: false } };
    const invoke = vi.fn(async () => found);
    installBridge(invoke);
    await expect(
      platform.agentRuntime?.discover({ userId: "u1", email: "a@b.test", cloudUrl: "https://cloud.test" })
    ).resolves.toEqual(found);
    await platform.agentRuntime?.discover({ userId: "u1", cloudUrl: "https://cloud.test" });
    expect(invoke).toHaveBeenCalledWith("daemon_discover", {
      appIdentity: { userId: "u1", email: "a@b.test", cloudUrl: "https://cloud.test" },
    });
    expect(invoke).toHaveBeenCalledWith("daemon_discover", {
      appIdentity: { userId: "u1", cloudUrl: "https://cloud.test" },
    });
  });

  it("lists the daemon's runners, reading none as empty", async () => {
    const runner = {
      name: "engine-ws-p",
      runner_id: "r1",
      workspace_slug: "ws",
      project_slug: "P",
      working_dir: "/t",
      agent: "engine",
    };
    const invoke = vi.fn(async () => [runner]);
    installBridge(invoke);
    await expect(platform.agentRuntime?.localRunners()).resolves.toEqual([runner]);
    expect(invoke).toHaveBeenCalledWith("daemon_runners", undefined);
    installBridge(vi.fn(async () => null));
    await expect(platform.agentRuntime?.localRunners()).resolves.toEqual([]);
  });

  it("adds an engine runner, sending no working folder when none is named", async () => {
    const invoke = vi.fn(async () => ({
      runner: "r",
      runner_id: "id",
      workspace: "ws",
      project: "P",
      working_dir: "/t",
    }));
    installBridge(invoke);
    await platform.agentRuntime?.addEngineRunner({ workspace: "ws", project: "P" });
    await platform.agentRuntime?.addEngineRunner({ workspace: "ws", project: "P", workingDir: "/w" });
    expect(invoke).toHaveBeenCalledWith("daemon_engine_runner_add", {
      workspace: "ws",
      project: "P",
      workingDir: null,
    });
    expect(invoke).toHaveBeenCalledWith("daemon_engine_runner_add", {
      workspace: "ws",
      project: "P",
      workingDir: "/w",
    });
  });

  it("runs the set-up, engine sign-in and credential reload commands", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await platform.agentRuntime?.installService();
    await platform.agentRuntime?.ensureEngine();
    await platform.agentRuntime?.engineLogin("dev-code");
    await platform.agentRuntime?.reloadCredentials();
    expect(invoke.mock.calls).toEqual([
      ["daemon_install_service", undefined],
      ["daemon_ensure_engine", undefined],
      ["pidash_cli_engine_login", { deviceCode: "dev-code" }],
      ["daemon_credentials_reload", undefined],
    ]);
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

  it("asks the daemon to reconnect its cloud connections", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await platform.agentRuntime?.requestReconnect();
    expect(invoke).toHaveBeenCalledWith("daemon_reconnect", undefined);
  });

  it("runs the migration and notices commands", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "legacy_scan") return [];
      if (
        command === "desktop_notices_load" ||
        command === "desktop_notices_record" ||
        command === "desktop_notices_dismiss"
      )
        return { notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] };
      return undefined;
    });
    installBridge(invoke);
    await expect(platform.agentRuntime?.scanLegacyTrees()).resolves.toEqual([]);
    await platform.agentRuntime?.finishLegacyTree({ dir: "/d", account: "u1", daemonAccount: "u1" });
    await platform.agentRuntime?.restoreLegacyFolder({ runner: "r", workingDir: "/w" });
    await platform.agentRuntime?.loadNotices("u1");
    await platform.agentRuntime?.recordNotices("u1", { legacy_folders: [], pod_member_projects: [] });
    await platform.agentRuntime?.dismissNotices("u1", ["folder:/t"]);
    await platform.agentRuntime?.signOutComputer();
    expect(invoke).toHaveBeenCalledWith("legacy_scan", undefined);
    expect(invoke).toHaveBeenCalledWith("legacy_finish", { dir: "/d", account: "u1", daemonAccount: "u1" });
    expect(invoke).toHaveBeenCalledWith("legacy_restore_folder", { runner: "r", workingDir: "/w" });
    expect(invoke).toHaveBeenCalledWith("desktop_notices_load", { account: "u1" });
    expect(invoke).toHaveBeenCalledWith("desktop_notices_record", {
      account: "u1",
      notices: { legacy_folders: [], pod_member_projects: [] },
    });
    expect(invoke).toHaveBeenCalledWith("desktop_notices_dismiss", { account: "u1", keys: ["folder:/t"] });
    expect(invoke).toHaveBeenCalledWith("pidash_cli_logout", { engine: false });
  });

  it("rejects runtime calls without the bridge", async () => {
    await expect(platform.agentRuntime?.discover({ userId: "u1", cloudUrl: "https://c.test" })).rejects.toThrow(
      "daemon_discover needs the native bridge"
    );
    await expect(platform.agentRuntime?.engineLogin("d")).rejects.toThrow(
      "pidash_cli_engine_login needs the native bridge"
    );
    await expect(platform.agentRuntime?.requestReconnect()).rejects.toThrow("daemon_reconnect needs the native bridge");
    await expect(platform.agentRuntime?.signOutComputer()).rejects.toThrow("pidash_cli_logout needs the native bridge");
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
