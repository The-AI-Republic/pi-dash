/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * "Set up Pi Dash on this computer" (design §4.7, PIDESKAPP-114): install the
 * CLI the app carries, sign it in with a device grant the app approves for the
 * workspace, install and start the service, install the engine — and nothing
 * at all unless the user agreed.
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

const HELLO = {
  daemon_version: "0.2.0",
  ipc_version: 6,
  cloud_url: "https://pidash.test",
  account: { user_id: "u-1", cloud_url: "https://pidash.test" },
  workspaces: [{ slug: "acme", signed_in: true, runners: 0 }],
  machine_id: "m-1",
  engine: "available",
};
let discovered: Record<string, unknown>;
let cliInstalled: boolean;

beforeEach(async () => {
  vi.resetModules();
  invoke.mockReset();
  request.mockReset();
  discovered = { kind: "no_daemon", cli: { installed: false } };
  cliInstalled = false;
  (window as unknown as Record<string, unknown>).__TAURI__ = { core: { invoke } };
  request.mockImplementation(async ({ url }: { url: string }) => {
    if (url === "/api/auth/get-csrf-token/") return { data: { csrf_token: "csrf-1" } };
    if (url === "/api/v1/auth/device/start/") return { data: { device_code: "dev-code-1", user_code: "ABCD1234" } };
    return { data: {} };
  });
  invoke.mockImplementation(async (command: string) => {
    if (command === "detect_pidash_cli")
      return cliInstalled
        ? { installed: true, path: "/home/dev/.local/bin/pidash", version: "0.2.0" }
        : { installed: false, path: null, version: null };
    if (command === "daemon_discover") return discovered;
    if (command === "daemon_install_service") discovered = { kind: "compatible", hello: HELLO };
    return undefined;
  });
  const daemon = await import("@/services/desktop-daemon");
  daemon.setAppIdentity({ userId: "u-1", cloudUrl: "https://pidash.test" });
});

const commands = () => invoke.mock.calls.map(([c]) => c as string).filter((c) => c !== "daemon_discover");

describe("setUpCliForThisMachine", () => {
  it("installs the CLI, signs it in for the workspace, then installs the service and the engine", async () => {
    const { setUpCliForThisMachine } = await import("@/services/pidash-cli");
    const steps: string[] = [];
    await setUpCliForThisMachine("acme", (step) => steps.push(step));

    expect(commands()).toEqual([
      "detect_pidash_cli",
      "install_pidash_cli",
      "pidash_cli_login",
      "daemon_install_service",
      "daemon_ensure_engine",
    ]);
    expect(steps).toEqual(["install_cli", "sign_in", "service", "engine", "done"]);
    // The approval is the half only a signed-in session can do, scoped to the workspace.
    const approve = request.mock.calls.find(([c]) => c.url === "/api/v1/auth/device/approve/");
    expect(approve?.[0].data).toEqual({ user_code: "ABCD1234", workspace_slug: "acme", interactive: false });
    // The CLI gets the *device* code, never the user code.
    const login = invoke.mock.calls.find(([c]) => c === "pidash_cli_login");
    expect(login?.[1]).toEqual({ deviceCode: "dev-code-1", cloudUrl: "https://pidash.test", workspace: "acme" });
  });

  it("skips the install when the CLI is there, and the sign-in when it is already this account", async () => {
    cliInstalled = true;
    discovered = { kind: "compatible", hello: HELLO };
    const { setUpCliForThisMachine } = await import("@/services/pidash-cli");
    await setUpCliForThisMachine("acme");
    expect(commands()).toEqual(["detect_pidash_cli", "daemon_install_service", "daemon_ensure_engine"]);
    expect(request).not.toHaveBeenCalled();
  });
});

describe("requestDaemonSetup", () => {
  it("sets nothing up when the user declines, and asks again next time", async () => {
    const { registerSetUpPrompt, requestDaemonSetup } = await import("@/services/pidash-cli");
    const asked: string[] = [];
    registerSetUpPrompt((req, done) => {
      asked.push(req.reason);
      done(false);
    });
    await expect(requestDaemonSetup("chat", "acme")).resolves.toBe(false);
    await expect(requestDaemonSetup("runner", "acme")).resolves.toBe(false);
    expect(asked).toEqual(["chat", "runner"]);
    expect(commands()).toEqual([]);
  });

  it("does not ask when the daemon is already the app's", async () => {
    discovered = { kind: "compatible", hello: HELLO };
    const { registerSetUpPrompt, requestDaemonSetup } = await import("@/services/pidash-cli");
    const prompt = vi.fn();
    registerSetUpPrompt(prompt);
    await expect(requestDaemonSetup("engine", "acme")).resolves.toBe(true);
    expect(prompt).not.toHaveBeenCalled();
  });

  it("never offers a second daemon when another account's is there", async () => {
    discovered = {
      kind: "account_mismatch",
      daemon_account: "x@example.com",
      daemon_cloud_url: "",
      hosts_runners: true,
    };
    const { registerSetUpPrompt, requestDaemonSetup } = await import("@/services/pidash-cli");
    const prompt = vi.fn();
    registerSetUpPrompt(prompt);
    await expect(requestDaemonSetup("engine", "acme")).resolves.toBe(false);
    expect(prompt).not.toHaveBeenCalled();
    expect(commands()).toEqual([]);
  });
});
