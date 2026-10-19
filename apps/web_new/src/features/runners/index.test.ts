// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  client: { baseUrl: "https://api.test" },
  agentRuntime: {},
  confirmRuntimeSignOut: vi.fn(async () => ({ deleteChatHistory: false })),
  disposeRuntimeForSignOut: vi.fn(async (..._args: Array<unknown>) => undefined),
  listen: vi.fn(async (_event: string, _handler: (event: { payload: unknown }) => void) => {
    harness.unlisten?.();
    return harness.unlisten ?? (() => undefined);
  }),
  unlisten: undefined as (() => void) | undefined,
}));

vi.mock("../../core/api/client.js", () => ({
  getAppClient: () => harness.client,
}));

vi.mock("../../core/platform/index.js", () => ({
  get platform() {
    return {
      kind: "desktop",
      agentRuntime: harness.agentRuntime,
      cliSetup: {
        detect: async () => ({ installed: true, path: "/usr/local/bin/pidash", version: "1.0.0" }),
        install: async () => ({ installed: true, path: "/usr/local/bin/pidash", version: "1.0.0" }),
        onInstallLog: (callback: (progress: { line: string }) => void) => {
          void harness.listen("pidash-install-log", (event) => {
            void callback(event.payload as { line: string });
          });
          return () => undefined;
        },
        login: async () => undefined,
      },
    };
  },
}));

vi.mock("./runtime/signOutPrompt.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./runtime/signOutPrompt.js")>();
  return {
    ...original,
    confirmRuntimeSignOut: () => harness.confirmRuntimeSignOut(),
  };
});

vi.mock("./runtime/service.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./runtime/service.js")>();
  return {
    ...original,
    disposeRuntimeForSignOut: (input: unknown) => harness.disposeRuntimeForSignOut(input),
  };
});

import { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "../../core/session/index.js";

describe("root runtime sign-out lifecycle", () => {
  afterEach(() => {
    registerRuntimeSignOutHooks(null);
    vi.clearAllMocks();
  });

  it("registers the runtime hooks on import", async () => {
    await import("./runtime/SignOutLifecycle.js");
    const hooks = getRuntimeSignOutHooks();
    expect(hooks).not.toBeNull();
    await hooks?.confirmRuntimeSignOut();
    expect(harness.confirmRuntimeSignOut).toHaveBeenCalled();
    await hooks?.disposeRuntimeForSignOut({ deleteChatHistory: true });
    expect(harness.disposeRuntimeForSignOut).toHaveBeenCalledWith({
      choice: { deleteChatHistory: true },
      client: harness.client,
      runtime: harness.agentRuntime,
    });
  });

  it("does not register hooks without a desktop runtime", async () => {
    const { registerRuntimeSignOutLifecycle } = await import("./runtime/SignOutLifecycle.js");
    registerRuntimeSignOutLifecycle(undefined);
    expect(getRuntimeSignOutHooks()).toBeNull();
  });

  it("exposes the command-line setup surface from the feature barrel", async () => {
    const feature = await import("./index.js");
    expect(feature.CLI_SETUP_HEADING).toBe("Command-line tool");
    expect(typeof feature.createSetupLock).toBe("function");
    expect(typeof feature.setUpCli).toBe("function");
    expect(feature.CliSetupPanel).toBeTypeOf("function");
  });
});
