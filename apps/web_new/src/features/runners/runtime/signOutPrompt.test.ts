// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import {
  beginSignOutPromptLoad,
  confirmRuntimeSignOut,
  failSignOutPromptLoad,
  registerSignOutPrompt,
} from "./signOutPrompt.js";

function track(): { promise: Promise<RuntimeSignOutChoice | null>; settled: () => boolean } {
  const state = { done: false };
  const promise = confirmRuntimeSignOut().finally(() => {
    state.done = true;
  });
  return { promise, settled: () => state.done };
}

afterEach(() => {
  registerSignOutPrompt(null);
});

describe("confirmRuntimeSignOut", () => {
  it("keeps the history when no dialog is expected", async () => {
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });

  it("waits while the dialog chunk is loading and asks it once registered", async () => {
    beginSignOutPromptLoad();
    beginSignOutPromptLoad();
    const pending = track();
    await Promise.resolve();
    expect(pending.settled()).toBe(false);
    registerSignOutPrompt((resolve) => resolve({ deleteChatHistory: true }));
    await expect(pending.promise).resolves.toEqual({ deleteChatHistory: true });
  });

  it("settles a queued request with the keep-history default when the fetch fails", async () => {
    beginSignOutPromptLoad();
    const pending = track();
    failSignOutPromptLoad();
    await expect(pending.promise).resolves.toEqual({ deleteChatHistory: false });
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });

  it("settles a queued request and later requests when the dialog goes away", async () => {
    registerSignOutPrompt(vi.fn());
    beginSignOutPromptLoad();
    registerSignOutPrompt(null);
    beginSignOutPromptLoad();
    const pending = track();
    registerSignOutPrompt(null);
    await expect(pending.promise).resolves.toEqual({ deleteChatHistory: false });
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });
});
