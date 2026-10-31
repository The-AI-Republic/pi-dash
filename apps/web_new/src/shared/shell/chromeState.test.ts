// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clampSidebarWidth,
  createSidebarPanelStore,
  loadSidebarPanel,
  MAIN_SIDEBAR_PANEL,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  sidebarPanelStorageKey,
  sidebarPanelStore,
  useShellChrome,
} from "./chromeState.js";

const MAIN_KEY = sidebarPanelStorageKey(MAIN_SIDEBAR_PANEL);

function resetMain(): void {
  useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false, sidebarWidth: SIDEBAR_DEFAULT_WIDTH });
  globalThis.localStorage.removeItem(MAIN_KEY);
}

beforeEach(() => {
  resetMain();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetMain();
});

describe("shell chrome state", () => {
  it("starts expanded with peek closed at the default width", () => {
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    expect(useShellChrome.getState().peekOpen).toBe(false);
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it("sets and flips the collapse flag", () => {
    useShellChrome.getState().setSidebarCollapsed(true);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    useShellChrome.getState().toggleSidebarCollapsed();
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    useShellChrome.getState().toggleSidebarCollapsed();
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("clears peek on any toggle", () => {
    useShellChrome.getState().setPeekOpen(true);
    useShellChrome.getState().toggleSidebarCollapsed();
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("opens and clears peek", () => {
    useShellChrome.getState().setPeekOpen(true);
    expect(useShellChrome.getState().peekOpen).toBe(true);
    useShellChrome.getState().clearPeek();
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("clamps dragged widths into the grip range", () => {
    useShellChrome.getState().setSidebarWidth(300);
    expect(useShellChrome.getState().sidebarWidth).toBe(300);
    useShellChrome.getState().setSidebarWidth(10_000);
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_MAX_WIDTH);
    useShellChrome.getState().setSidebarWidth(1);
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_MIN_WIDTH);
  });

  it("persists the snapshot so a reload restores it", async () => {
    useShellChrome.getState().setSidebarCollapsed(true);
    useShellChrome.getState().setPeekOpen(true);
    useShellChrome.getState().setSidebarWidth(300);
    const raw = globalThis.localStorage.getItem(MAIN_KEY);
    expect(raw).toContain('"sidebarCollapsed":true');
    expect(raw).toContain('"peekOpen":true');
    expect(raw).toContain('"sidebarWidth":300');

    vi.resetModules();
    const reloaded = await import("./chromeState.js");
    expect(reloaded.useShellChrome.getState().sidebarCollapsed).toBe(true);
    expect(reloaded.useShellChrome.getState().peekOpen).toBe(true);
    expect(reloaded.useShellChrome.getState().sidebarWidth).toBe(300);
    reloaded.useShellChrome.setState({
      sidebarCollapsed: false,
      peekOpen: false,
      sidebarWidth: SIDEBAR_DEFAULT_WIDTH,
    });
  });

  it.each([
    "{oops",
    "42",
    "null",
    '{"sidebarCollapsed":"yes","peekOpen":false,"sidebarWidth":300}',
    '{"sidebarCollapsed":true,"peekOpen":"no","sidebarWidth":300}',
    '{"sidebarCollapsed":true,"peekOpen":false,"sidebarWidth":"wide"}',
    '{"sidebarCollapsed":true,"peekOpen":false}',
  ])("falls back to defaults on the stored payload %s", async (payload) => {
    globalThis.localStorage.setItem(MAIN_KEY, payload);
    vi.resetModules();
    const reloaded = await import("./chromeState.js");
    expect(reloaded.useShellChrome.getState().sidebarCollapsed).toBe(false);
    expect(reloaded.useShellChrome.getState().peekOpen).toBe(false);
    expect(reloaded.useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it("clamps a stored width that drifted outside the range", () => {
    globalThis.localStorage.setItem(MAIN_KEY, '{"sidebarCollapsed":false,"peekOpen":false,"sidebarWidth":10000}');
    expect(loadSidebarPanel(MAIN_SIDEBAR_PANEL).sidebarWidth).toBe(SIDEBAR_MAX_WIDTH);
  });

  it("keys panels independently of each other", () => {
    expect(sidebarPanelStorageKey("main")).toBe("pidash:sidebar-panel:main");
    expect(sidebarPanelStorageKey("extended")).toBe("pidash:sidebar-panel:extended");
    const extended = createSidebarPanelStore("extended");
    extended.getState().setSidebarCollapsed(true);
    extended.getState().setSidebarWidth(340);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(globalThis.localStorage.getItem("pidash:sidebar-panel:extended")).toContain('"sidebarWidth":340');
    expect(globalThis.localStorage.getItem(MAIN_KEY)).toBeNull();
    globalThis.localStorage.removeItem("pidash:sidebar-panel:extended");
  });

  it("shares one store per panel id, main included", () => {
    expect(sidebarPanelStore(MAIN_SIDEBAR_PANEL)).toBe(useShellChrome);
    const first = sidebarPanelStore("profile");
    const second = sidebarPanelStore("profile");
    expect(second).toBe(first);
    expect(first).not.toBe(useShellChrome);
    first.getState().setSidebarCollapsed(true);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    first.setState({ sidebarCollapsed: false, peekOpen: false, sidebarWidth: SIDEBAR_DEFAULT_WIDTH });
    globalThis.localStorage.removeItem(sidebarPanelStorageKey("profile"));
  });

  it("rests foreign widths at the default", () => {
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(clampSidebarWidth(Number.POSITIVE_INFINITY)).toBe(SIDEBAR_DEFAULT_WIDTH);
  });
});
