/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The app can turn this machine into a runner host without a terminal: install
 * the bundled CLI, then sign it in with a device-code grant it approves with
 * its own session.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const request = vi.fn();

vi.mock("@/services/agent-runtime", () => ({ isDesktop: () => true }));
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

beforeEach(() => {
  invoke.mockReset();
  request.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI__ = { core: { invoke } };
  request.mockImplementation(async ({ url }: { url: string }) => {
    if (url === "/api/auth/get-csrf-token/") return { data: { csrf_token: "csrf-1" } };
    if (url === "/api/v1/auth/device/start/") return { data: { device_code: "dev-code-1", user_code: "ABCD1234" } };
    return { data: {} };
  });
});

describe("setUpCliForThisMachine", () => {
  it("installs the CLI, approves its own grant, and hands the code to the CLI", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "detect_pidash_cli") return { installed: false, path: null, version: null };
      return undefined;
    });
    const { setUpCliForThisMachine } = await import("@/services/pidash-cli");
    await setUpCliForThisMachine("acme");

    expect(invoke.mock.calls.map(([c]) => c)).toEqual(["detect_pidash_cli", "install_pidash_cli", "pidash_cli_login"]);
    // The approval is the half only a signed-in session can do.
    const approve = request.mock.calls.find(([c]) => c.url === "/api/v1/auth/device/approve/");
    expect(approve?.[0].data).toEqual({ user_code: "ABCD1234" });
    // The CLI gets the *device* code, never the user code.
    const login = invoke.mock.calls.find(([c]) => c === "pidash_cli_login");
    expect(login?.[1]).toEqual({
      deviceCode: "dev-code-1",
      cloudUrl: "https://pidash.test",
      workspace: "acme",
    });
  });

  it("skips the install when the CLI is already there", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "detect_pidash_cli")
        return { installed: true, path: "/home/dev/.local/bin/pidash", version: "0.1.23" };
      return undefined;
    });
    const { setUpCliForThisMachine } = await import("@/services/pidash-cli");
    await setUpCliForThisMachine("acme");
    expect(invoke.mock.calls.map(([c]) => c)).toEqual(["detect_pidash_cli", "pidash_cli_login"]);
  });
});
