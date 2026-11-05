/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The app as a client of this computer's daemon (design §4.3, §4.4, §4.6):
 * what the engine sign-in state allows, and that the engine is renewed
 * silently only when it is `expiring` or `expired`.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const request = vi.fn();

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "https://pidash.test" }));
vi.mock("@/pi-dash-web/components/desktop/agent-runtime-edition", () => ({
  CSRF_TOKEN_PATH: "/api/auth/get-csrf-token/",
  AGENT_RUNTIME_REASON_MESSAGES: {},
}));
vi.mock("@pi-dash/services", () => ({
  APIService: class {
    request = request;
  },
}));

type Daemon = typeof import("@/services/desktop-daemon");
let daemon: Daemon;
let signInState: string;
let discoveryKind: string;

function hello() {
  return {
    daemon_version: "0.2.0",
    ipc_version: 6,
    cloud_url: "https://pidash.test",
    account: { user_id: "u-1", cloud_url: "https://pidash.test" },
    workspaces: [{ slug: "acme", signed_in: true, runners: 1 }],
    machine_id: "m-1",
    engine: "available",
    engine_detail: { state: "available", sign_in_state: signInState },
  };
}

beforeEach(async () => {
  vi.resetModules();
  invoke.mockReset();
  request.mockReset();
  signInState = "active";
  discoveryKind = "compatible";
  (window as unknown as Record<string, unknown>).__TAURI__ = { core: { invoke } };
  request.mockImplementation(async ({ url }: { url: string }) => {
    if (url === "/api/auth/get-csrf-token/") return { data: { csrf_token: "csrf-1" } };
    if (url === "/api/v1/auth/device/start/") return { data: { device_code: "engine-code", user_code: "WXYZ" } };
    return { data: {} };
  });
  invoke.mockImplementation(async (command: string) => {
    if (command === "daemon_discover")
      return discoveryKind === "compatible"
        ? { kind: "compatible", hello: hello() }
        : { kind: discoveryKind, daemon_account: "other@example.com", daemon_cloud_url: "", hosts_runners: false };
    return undefined;
  });
  daemon = await import("@/services/desktop-daemon");
  daemon.setAppIdentity({ userId: "u-1", cloudUrl: "https://pidash.test" });
});

const engineLogins = () => invoke.mock.calls.filter(([c]) => c === "pidash_cli_engine_login");

describe("engine sign-in rules", () => {
  it("maps each sign-in state to what the app may do", () => {
    expect(daemon.engineActionFor("expiring")).toBe("renew_silently");
    expect(daemon.engineActionFor("expired")).toBe("renew_silently");
    expect(daemon.engineActionFor("never")).toBe("turn_on");
    expect(daemon.engineActionFor("signed_out")).toBe("sign_in");
    expect(daemon.engineActionFor("revoked")).toBe("sign_in");
    expect(daemon.engineActionFor("active")).toBe("none");
    expect(daemon.engineActionFor(undefined)).toBe("none");
  });

  for (const state of ["expiring", "expired"]) {
    it(`renews silently when the engine is ${state}: an engine grant approved by the session, code on stdin`, async () => {
      signInState = state;
      await daemon.discoverDaemon();
      await expect(daemon.renewEngineIfAllowed(1_000_000)).resolves.toBe(true);
      const start = request.mock.calls.find(([c]) => c.url === "/api/v1/auth/device/start/");
      expect(start?.[0].data).toEqual({ scope: "engine" });
      const approve = request.mock.calls.find(([c]) => c.url === "/api/v1/auth/device/approve/");
      expect(approve?.[0].data).toEqual({ user_code: "WXYZ", interactive: false });
      expect(engineLogins()).toEqual([["pidash_cli_engine_login", { deviceCode: "engine-code" }]]);
    });
  }

  for (const state of ["active", "never", "signed_out", "revoked"]) {
    it(`never renews silently when the engine is ${state}`, async () => {
      signInState = state;
      await daemon.discoverDaemon();
      await expect(daemon.renewEngineIfAllowed(1_000_000)).resolves.toBe(false);
      expect(request).not.toHaveBeenCalled();
      expect(engineLogins()).toEqual([]);
    });
  }

  it("does not renew for a daemon signed in as someone else", async () => {
    signInState = "expired";
    discoveryKind = "account_mismatch";
    await daemon.discoverDaemon();
    await expect(daemon.renewEngineIfAllowed(1_000_000)).resolves.toBe(false);
    expect(engineLogins()).toEqual([]);
  });

  it("does not retry a failed silent renewal on every check", async () => {
    signInState = "expired";
    await daemon.discoverDaemon();
    invoke.mockImplementation(async (command: string) => {
      if (command === "daemon_discover") return { kind: "compatible", hello: hello() };
      if (command === "pidash_cli_engine_login") throw new Error("server refused");
      return undefined;
    });
    await expect(daemon.renewEngineIfAllowed(1_000_000)).resolves.toBe(false);
    await expect(daemon.renewEngineIfAllowed(1_000_000 + 60_000)).resolves.toBe(false);
    expect(engineLogins()).toHaveLength(1);
    await expect(daemon.renewEngineIfAllowed(1_000_000 + 11 * 60_000)).resolves.toBe(false);
    expect(engineLogins()).toHaveLength(2);
  });

  it("an explicit sign-in (turn on, or after a sign-out) is marked interactive", async () => {
    signInState = "signed_out";
    await daemon.discoverDaemon();
    await daemon.signInEngineExplicitly();
    const approve = request.mock.calls.find(([c]) => c.url === "/api/v1/auth/device/approve/");
    expect(approve?.[0].data).toEqual({ user_code: "WXYZ", interactive: true });
    expect(engineLogins()).toHaveLength(1);
  });

  it("refuses an explicit engine sign-in on another account's daemon", async () => {
    discoveryKind = "account_mismatch";
    await daemon.discoverDaemon();
    await expect(daemon.signInEngineExplicitly()).rejects.toThrow("signed in as other@example.com");
    expect(request).not.toHaveBeenCalled();
  });
});

describe("discovery", () => {
  it("is evaluated for the signed-in app account and forgotten on an account switch", async () => {
    await daemon.discoverDaemon();
    expect(invoke).toHaveBeenCalledWith("daemon_discover", {
      appIdentity: { userId: "u-1", cloudUrl: "https://pidash.test" },
    });
    expect(daemon.compatibleHello()).not.toBeNull();
    daemon.setAppIdentity({ userId: "u-2", cloudUrl: "https://pidash.test" });
    expect(daemon.currentDiscovery()).toBeNull();
  });

  it("names every reason the engine features are off", () => {
    const cli = { installed: false, path: null, version: null, updatable: false };
    expect(daemon.discoveryProblem({ kind: "no_daemon", cli })).toMatch(/not set up/);
    expect(daemon.discoveryProblem({ kind: "too_old", cli })).toMatch(/too old/);
    expect(daemon.discoveryProblem({ kind: "not_answering", reason: "timed out" })).toMatch(/not answering/);
    expect(daemon.discoveryProblem({ kind: "app_too_old", daemon_version: "9.0.0", ipc_version: 9 })).toMatch(
      /Update the app/
    );
    expect(
      daemon.discoveryProblem({
        kind: "account_mismatch",
        daemon_account: "x@example.com",
        daemon_cloud_url: "",
        hosts_runners: false,
      })
    ).toBe("Pi Dash on this computer is signed in as x@example.com.");
  });
});
