// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { THEME_PALETTE_STORAGE_KEY, THEME_STORAGE_KEY } from "../../core/theme/index.js";
import { markInstanceBootScheme } from "./theme.js";

afterEach(() => {
  localStorage.removeItem(THEME_STORAGE_KEY);
  localStorage.removeItem(THEME_PALETTE_STORAGE_KEY);
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.style.removeProperty("color-scheme");
  vi.restoreAllMocks();
});

describe("markInstanceBootScheme", () => {
  it("marks the document with the stored dark choice before the router mounts", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "dark");
    expect(markInstanceBootScheme()).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
  });

  it("marks an explicit light choice", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "light");
    expect(markInstanceBootScheme()).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("writes to a provided target", () => {
    const target = document.createElement("div");
    localStorage.setItem(THEME_STORAGE_KEY, "dark-contrast");
    expect(markInstanceBootScheme(target)).toBe("dark-contrast");
    expect(target.getAttribute("data-theme")).toBe("dark-contrast");
    expect(document.documentElement.getAttribute("data-theme")).toBeNull();
  });

  it("keeps a stored custom palette so the boot art follows its dark half", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "custom");
    localStorage.setItem(
      THEME_PALETTE_STORAGE_KEY,
      JSON.stringify({ primary: "#112233", background: "#445566", darkPalette: true })
    );
    const target = document.createElement("div");
    expect(markInstanceBootScheme(target)).toBe("custom");
    expect(target.getAttribute("data-theme")).toBe("custom");
    expect(target.style.getPropertyValue("color-scheme")).toBe("dark");
    expect(target.style.getPropertyValue("--theme-custom-mode")).toBe("dark");
  });

  it("follows the OS when no choice is stored", () => {
    vi.spyOn(window, "matchMedia").mockReturnValue({
      matches: true,
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    });
    expect(markInstanceBootScheme()).toBe("dark");
  });
});
