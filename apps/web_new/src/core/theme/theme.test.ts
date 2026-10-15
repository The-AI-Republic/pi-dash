// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildThemeBag,
  isDarkValue,
  parseThemeValue,
  readStoredTheme,
  readThemeBag,
  resolveScheme,
  THEME_OPTIONS,
  writeStoredTheme,
} from "./theme.js";

describe("theme model", () => {
  it("offers the six stored values with human labels", () => {
    expect(THEME_OPTIONS.map((option) => option.value)).toEqual([
      "system",
      "light",
      "dark",
      "light-contrast",
      "dark-contrast",
      "custom",
    ]);
    expect(THEME_OPTIONS.map((option) => option.label)).toEqual([
      "System preference",
      "Light",
      "Dark",
      "Light high contrast",
      "Dark high contrast",
      "Custom theme",
    ]);
  });

  it("resolves explicit values literally and system from the OS", () => {
    expect(resolveScheme("light", true)).toBe("light");
    expect(resolveScheme("dark", false)).toBe("dark");
    expect(resolveScheme("light-contrast", true)).toBe("light-contrast");
    expect(resolveScheme("dark-contrast", false)).toBe("dark-contrast");
    expect(resolveScheme("custom", true)).toBe("custom");
    expect(resolveScheme("system", true)).toBe("dark");
    expect(resolveScheme("system", false)).toBe("light");
  });

  it("classifies dark bases for icon and art swaps", () => {
    expect(isDarkValue("dark", false)).toBe(true);
    expect(isDarkValue("dark-contrast", false)).toBe(true);
    expect(isDarkValue("light", true)).toBe(false);
    expect(isDarkValue("light-contrast", true)).toBe(false);
    expect(isDarkValue("system", true)).toBe(true);
    expect(isDarkValue("system", false)).toBe(false);
    expect(isDarkValue("custom", true)).toBe(true);
    expect(isDarkValue("custom", false)).toBe(false);
  });

  it("falls back to system for unknown stored values", () => {
    expect(parseThemeValue("dark")).toBe("dark");
    expect(parseThemeValue("sepia")).toBe("system");
    expect(parseThemeValue(null)).toBe("system");
    expect(parseThemeValue(undefined)).toBe("system");
    expect(parseThemeValue(42)).toBe("system");
  });

  it("reads the profile bag and builds it back", () => {
    expect(readThemeBag(null)).toEqual({ value: "system" });
    expect(readThemeBag(undefined)).toEqual({ value: "system" });
    expect(readThemeBag({ theme: "dark" })).toEqual({ value: "dark" });
    expect(readThemeBag({ theme: "nope" })).toEqual({ value: "system" });
    expect(readThemeBag({})).toEqual({ value: "system" });
    expect(readThemeBag({ theme: "custom" })).toEqual({ value: "custom" });
    expect(readThemeBag({ theme: "custom", primary: "#ffffff" })).toEqual({ value: "custom" });
    expect(readThemeBag({ theme: "custom", primary: "#ffffff", background: "#000000", darkPalette: true })).toEqual({
      value: "custom",
      custom: { primary: "#ffffff", background: "#000000", darkPalette: true },
    });
    expect(readThemeBag({ theme: "custom", primary: "#ffffff", background: "#000000", darkPalette: "yes" })).toEqual({
      value: "custom",
      custom: { primary: "#ffffff", background: "#000000", darkPalette: false },
    });
    expect(buildThemeBag("light", undefined)).toEqual({ theme: "light" });
    expect(buildThemeBag("custom", undefined)).toEqual({ theme: "custom" });
    expect(buildThemeBag("custom", { primary: "#a", background: "#b", darkPalette: false }, undefined)).toEqual({
      theme: "custom",
      primary: "#a",
      background: "#b",
      darkPalette: false,
    });
  });

  it("carries the cached bag over so theme switches keep a saved custom palette", () => {
    const cached = { theme: "custom", primary: "#a", background: "#b", darkPalette: true };
    expect(buildThemeBag("dark", undefined, cached)).toEqual({
      theme: "dark",
      primary: "#a",
      background: "#b",
      darkPalette: true,
    });
    expect(buildThemeBag("custom", { primary: "#c", background: "#d", darkPalette: false }, cached)).toEqual({
      theme: "custom",
      primary: "#c",
      background: "#d",
      darkPalette: false,
    });
  });

  describe("stored choice", () => {
    afterEach(() => {
      vi.unstubAllGlobals();
    });

    it("reads the stored value and falls back without storage", () => {
      vi.stubGlobal("localStorage", { getItem: () => "dark" });
      expect(readStoredTheme()).toBe("dark");
      vi.stubGlobal("localStorage", { getItem: () => "sepia" });
      expect(readStoredTheme()).toBe("system");
      vi.stubGlobal("localStorage", undefined);
      expect(readStoredTheme()).toBe("system");
    });

    it("survives denied storage in both directions", () => {
      vi.stubGlobal("localStorage", {
        getItem: () => {
          throw new Error("denied");
        },
        setItem: () => {
          throw new Error("denied");
        },
      });
      expect(readStoredTheme()).toBe("system");
      expect(() => writeStoredTheme("dark")).not.toThrow();
      vi.stubGlobal("localStorage", undefined);
      expect(() => writeStoredTheme("dark")).not.toThrow();
    });

    it("writes the choice through", () => {
      const setItem = vi.fn();
      vi.stubGlobal("localStorage", { getItem: () => null, setItem });
      writeStoredTheme("light-contrast");
      expect(setItem).toHaveBeenCalledWith("theme", "light-contrast");
    });

    it("survives storage access denial", () => {
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
      Object.defineProperty(globalThis, "localStorage", {
        get() {
          throw new Error("denied");
        },
        configurable: true,
      });
      try {
        expect(readStoredTheme()).toBe("system");
        expect(() => writeStoredTheme("dark")).not.toThrow();
      } finally {
        if (descriptor) Object.defineProperty(globalThis, "localStorage", descriptor);
        else delete (globalThis as Record<string, unknown>)["localStorage"];
      }
    });
  });
});
