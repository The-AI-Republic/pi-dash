// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "./signOutHooks.js";

describe("runtime sign-out hooks", () => {
  afterEach(() => {
    registerRuntimeSignOutHooks(null);
  });

  it("starts empty", () => {
    expect(getRuntimeSignOutHooks()).toBeNull();
  });

  it("registers and unregisters", () => {
    const hooks = {
      confirmRuntimeSignOut: vi.fn(async () => ({ deleteChatHistory: false })),
      disposeRuntimeForSignOut: vi.fn(async () => undefined),
    };
    const unregister = registerRuntimeSignOutHooks(hooks);
    expect(getRuntimeSignOutHooks()).toBe(hooks);
    unregister();
    expect(getRuntimeSignOutHooks()).toBeNull();
  });

  it("replaces the previous registration", () => {
    const first = {
      confirmRuntimeSignOut: vi.fn(async () => ({ deleteChatHistory: false })),
      disposeRuntimeForSignOut: vi.fn(async () => undefined),
    };
    const second = {
      confirmRuntimeSignOut: vi.fn(async () => null),
      disposeRuntimeForSignOut: vi.fn(async () => undefined),
    };
    const unregisterFirst = registerRuntimeSignOutHooks(first);
    registerRuntimeSignOutHooks(second);
    expect(getRuntimeSignOutHooks()).toBe(second);
    unregisterFirst();
    expect(getRuntimeSignOutHooks()).toBe(second);
  });
});
