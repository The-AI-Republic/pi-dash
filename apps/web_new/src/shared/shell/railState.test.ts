// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isRailUndocked, useRailState } from "./railState.js";

const STORAGE_KEY = "pidash:rail";

function resetStore(): void {
  useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
  globalThis.localStorage.removeItem(STORAGE_KEY);
}

beforeEach(() => {
  resetStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetStore();
});

describe("rail state", () => {
  it("starts captioned and docked", () => {
    expect(useRailState.getState().density).toBe("labeled");
    expect(isRailUndocked(useRailState.getState(), "acme")).toBe(false);
  });

  it("switches density", () => {
    useRailState.getState().setDensity("icons");
    expect(useRailState.getState().density).toBe("icons");
    useRailState.getState().setDensity("labeled");
    expect(useRailState.getState().density).toBe("labeled");
  });

  it("undocks one workspace without touching the others", () => {
    useRailState.getState().setUndocked("acme", true);
    expect(isRailUndocked(useRailState.getState(), "acme")).toBe(true);
    expect(isRailUndocked(useRailState.getState(), "globex")).toBe(false);
    useRailState.getState().setUndocked("acme", false);
    expect(isRailUndocked(useRailState.getState(), "acme")).toBe(false);
  });

  it("persists density and docking so a reload restores them", async () => {
    useRailState.getState().setDensity("icons");
    useRailState.getState().setUndocked("acme", true);
    const raw = globalThis.localStorage.getItem(STORAGE_KEY);
    expect(raw).toContain('"density":"icons"');
    expect(raw).toContain('"acme":true');

    vi.resetModules();
    const reloaded = await import("./railState.js");
    expect(reloaded.useRailState.getState().density).toBe("icons");
    expect(reloaded.isRailUndocked(reloaded.useRailState.getState(), "acme")).toBe(true);
    reloaded.useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
  });

  it.each([
    "{oops",
    "7",
    "null",
    '{"density":"wide","undockedByWorkspace":{}}',
    '{"density":"icons","undockedByWorkspace":7}',
    '{"density":"icons","undockedByWorkspace":null}',
    '{"density":"icons","undockedByWorkspace":{"acme":"yes"}}',
  ])("falls back to defaults on the stored payload %s", async (payload) => {
    globalThis.localStorage.setItem(STORAGE_KEY, payload);
    vi.resetModules();
    const reloaded = await import("./railState.js");
    expect(reloaded.useRailState.getState().density).toBe("labeled");
    expect(reloaded.isRailUndocked(reloaded.useRailState.getState(), "acme")).toBe(false);
  });
});
