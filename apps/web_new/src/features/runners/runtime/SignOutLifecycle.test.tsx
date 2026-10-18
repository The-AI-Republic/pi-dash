// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  client: { baseUrl: "https://api.test" },
  agentRuntime: {},
  confirmRuntimeSignOut: vi.fn(async () => ({ deleteChatHistory: false })),
  registerSignOutPrompt: vi.fn(),
  disposeRuntimeForSignOut: vi.fn(async (..._args: Array<unknown>) => undefined),
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => harness.client,
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return { kind: "desktop", agentRuntime: harness.agentRuntime };
  },
}));

vi.mock("./signOutPrompt.js", () => ({
  attachSignOutPromptHost: vi.fn(),
  beginSignOutPromptLoad: vi.fn(),
  detachSignOutPromptHost: vi.fn(),
  confirmRuntimeSignOut: harness.confirmRuntimeSignOut,
  failSignOutPromptLoad: vi.fn(),
  registerSignOutPrompt: harness.registerSignOutPrompt,
}));

vi.mock("./service.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./service.js")>();
  return {
    ...original,
    disposeRuntimeForSignOut: (input: unknown) => harness.disposeRuntimeForSignOut(input),
  };
});

import { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "../../../core/session/index.js";
import { registerRuntimeSignOutLifecycle, RuntimeSignOutLifecycle } from "./SignOutLifecycle.js";

afterEach(() => {
  registerRuntimeSignOutHooks(null);
  vi.clearAllMocks();
});

describe("RuntimeSignOutLifecycle", () => {
  it("registers the hooks on import and delegates confirmation and disposal", async () => {
    const hooks = getRuntimeSignOutHooks();
    expect(hooks).not.toBeNull();
    await expect(hooks?.confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
    await expect(hooks?.confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
    expect(harness.confirmRuntimeSignOut).toHaveBeenCalledTimes(2);
    await hooks?.disposeRuntimeForSignOut({ deleteChatHistory: true });
    expect(harness.disposeRuntimeForSignOut).toHaveBeenCalledWith({
      choice: { deleteChatHistory: true },
      client: harness.client,
      runtime: harness.agentRuntime,
    });
  });

  it("does not register hooks without a desktop runtime", () => {
    registerRuntimeSignOutLifecycle(undefined);
    expect(getRuntimeSignOutHooks()).toBeNull();
  });

  it("mounts one prompt registration for the life of the app", async () => {
    const { unmount } = render(<RuntimeSignOutLifecycle />);
    await vi.waitFor(() => expect(harness.registerSignOutPrompt).toHaveBeenCalledTimes(1));
    expect(harness.registerSignOutPrompt).toHaveBeenCalledWith(expect.any(Function));
    unmount();
    expect(harness.registerSignOutPrompt).toHaveBeenLastCalledWith(null);
  });
});
