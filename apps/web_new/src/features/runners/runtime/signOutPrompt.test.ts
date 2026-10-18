// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import {
  attachSignOutPromptHost,
  beginSignOutPromptLoad,
  confirmRuntimeSignOut,
  detachSignOutPromptHost,
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

describe("confirmation host lifetime", () => {
  it("settles a queued request when its host unmounts before registering the dialog", async () => {
    attachSignOutPromptHost();
    beginSignOutPromptLoad();
    const pending = track();
    detachSignOutPromptHost();
    await Promise.resolve();
    expect(pending.settled()).toBe(false);
    await new Promise((resolve) => setTimeout(resolve, 0));
    await expect(pending.promise).resolves.toEqual({ deleteChatHistory: false });
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });

  it("keeps a queued request waiting across a remount in the same commit", async () => {
    attachSignOutPromptHost();
    beginSignOutPromptLoad();
    const pending = track();
    detachSignOutPromptHost();
    attachSignOutPromptHost();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(pending.settled()).toBe(false);
    registerSignOutPrompt((resolve) => resolve(null));
    await expect(pending.promise).resolves.toBeNull();
    detachSignOutPromptHost();
  });

  it("leaves other hosts in charge while one of them is still mounted", async () => {
    attachSignOutPromptHost();
    attachSignOutPromptHost();
    beginSignOutPromptLoad();
    const pending = track();
    detachSignOutPromptHost();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(pending.settled()).toBe(false);
    registerSignOutPrompt((resolve) => resolve({ deleteChatHistory: true }));
    await expect(pending.promise).resolves.toEqual({ deleteChatHistory: true });
    detachSignOutPromptHost();
    registerSignOutPrompt(null);
  });
});
