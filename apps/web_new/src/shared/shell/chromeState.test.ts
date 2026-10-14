// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useShellChrome } from "./chromeState.js";

const STORAGE_KEY = "pidash:shell-chrome";

function resetStore(): void {
  useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false });
  globalThis.localStorage.removeItem(STORAGE_KEY);
}

beforeEach(() => {
  resetStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetStore();
});

describe("shell chrome state", () => {
  it("starts expanded with peek closed", () => {
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("sets and flips the collapse flag", () => {
    useShellChrome.getState().setSidebarCollapsed(true);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    useShellChrome.getState().toggleSidebarCollapsed();
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    useShellChrome.getState().toggleSidebarCollapsed();
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("opens and clears peek", () => {
    useShellChrome.getState().setPeekOpen(true);
    expect(useShellChrome.getState().peekOpen).toBe(true);
    useShellChrome.getState().clearPeek();
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("persists both flags so a reload restores them", async () => {
    useShellChrome.getState().setSidebarCollapsed(true);
    useShellChrome.getState().setPeekOpen(true);
    const raw = globalThis.localStorage.getItem(STORAGE_KEY);
    expect(raw).toContain('"sidebarCollapsed":true');
    expect(raw).toContain('"peekOpen":true');

    vi.resetModules();
    const reloaded = await import("./chromeState.js");
    expect(reloaded.useShellChrome.getState().sidebarCollapsed).toBe(true);
    expect(reloaded.useShellChrome.getState().peekOpen).toBe(true);
    reloaded.useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false });
  });
});
