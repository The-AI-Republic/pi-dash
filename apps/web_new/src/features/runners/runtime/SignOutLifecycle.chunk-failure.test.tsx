// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  confirmRuntimeSignOut: vi.fn(async () => ({ deleteChatHistory: false })),
  registerSignOutPrompt: vi.fn(),
}));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, kind: "desktop", agentRuntime: {} } };
});

vi.mock("./signOutPrompt.js", () => ({
  beginSignOutPromptLoad: vi.fn(),
  confirmRuntimeSignOut: harness.confirmRuntimeSignOut,
  failSignOutPromptLoad: vi.fn(),
  registerSignOutPrompt: harness.registerSignOutPrompt,
}));

vi.mock("./SignOutDialog.js", () => {
  throw new Error("chunk unavailable");
});

import { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "../../../core/session/index.js";
import { RuntimeSignOutLifecycle } from "./SignOutLifecycle.js";

afterEach(() => {
  registerRuntimeSignOutHooks(null);
  vi.clearAllMocks();
});

describe("RuntimeSignOutLifecycle without its dialog chunk", () => {
  it("keeps the history default instead of trapping the user", async () => {
    await expect(getRuntimeSignOutHooks()?.confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
    expect(harness.confirmRuntimeSignOut).toHaveBeenCalledTimes(1);
    expect(harness.registerSignOutPrompt).not.toHaveBeenCalled();
  });

  it("renders nothing and registers no prompt", async () => {
    const { container } = render(<RuntimeSignOutLifecycle />);
    await vi.waitFor(() => expect(harness.registerSignOutPrompt).not.toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText("Sign out?")).not.toBeInTheDocument();
  });
});
