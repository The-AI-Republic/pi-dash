// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resetAdminStores } from "./stores.js";
import { ADMIN_THEME_KEY, applyStoredAdminTheme, readStoredAdminTheme, useAdminThemeStore } from "./theme.js";

function appliedScheme(): { theme: string | null; colorScheme: string } {
  return {
    theme: document.documentElement.getAttribute("data-theme"),
    colorScheme: document.documentElement.style.getPropertyValue("color-scheme"),
  };
}

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.style.removeProperty("color-scheme");
  resetAdminStores();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("readStoredAdminTheme", () => {
  it("falls back to system when nothing is stored", () => {
    expect(readStoredAdminTheme()).toBe("system");
  });

  it("reads each stored mode", () => {
    for (const mode of ["light", "dark", "system"] as const) {
      window.localStorage.setItem(ADMIN_THEME_KEY, mode);
      expect(readStoredAdminTheme()).toBe(mode);
    }
  });

  it("falls back to system for unknown values", () => {
    window.localStorage.setItem(ADMIN_THEME_KEY, "neon");
    expect(readStoredAdminTheme()).toBe("system");
  });
});

describe("useAdminThemeStore", () => {
  it("starts from the stored mode", async () => {
    window.localStorage.setItem(ADMIN_THEME_KEY, "dark");
    vi.resetModules();
    const fresh = await import("./theme.js");
    expect(fresh.useAdminThemeStore.getState().mode).toBe("dark");
  });

  it("persists and applies an explicit mode", () => {
    useAdminThemeStore.getState().setMode("dark");
    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("dark");
    expect(useAdminThemeStore.getState().mode).toBe("dark");
    expect(appliedScheme()).toEqual({ theme: "dark", colorScheme: "dark" });
  });

  it("resolves system through the OS preference", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({ matches: query === "(prefers-color-scheme: dark)", media: query }))
    );
    useAdminThemeStore.getState().setMode("system");
    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("system");
    expect(appliedScheme()).toEqual({ theme: "dark", colorScheme: "dark" });
  });

  it("resolves system to light when the OS preference is unreadable", () => {
    useAdminThemeStore.getState().setMode("system");
    expect(appliedScheme()).toEqual({ theme: "light", colorScheme: "light" });
  });

  it("resets to system through the admin registry", () => {
    useAdminThemeStore.getState().setMode("dark");
    resetAdminStores();
    expect(useAdminThemeStore.getState().mode).toBe("system");
    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("system");
    expect(appliedScheme()).toEqual({ theme: "light", colorScheme: "light" });
  });
});

describe("applyStoredAdminTheme", () => {
  it("applies the persisted mode to the document", () => {
    window.localStorage.setItem(ADMIN_THEME_KEY, "dark");
    applyStoredAdminTheme();
    expect(appliedScheme()).toEqual({ theme: "dark", colorScheme: "dark" });
  });
});
