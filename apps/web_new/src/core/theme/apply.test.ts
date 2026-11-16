// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi, afterEach } from "vitest";

import { applyResolvedTheme, clearCustomPalette, prefersDarkScheme } from "./apply.js";

describe("theme application", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("data-theme");
    clearCustomPalette(document.documentElement);
  });

  it("marks the resolved scheme and follows dark bases with color-scheme", () => {
    applyResolvedTheme("light", undefined, document.documentElement);
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("light");
    applyResolvedTheme("dark", undefined, document.documentElement);
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
    applyResolvedTheme("dark-contrast", undefined, document.documentElement);
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
    applyResolvedTheme("light-contrast", undefined, document.documentElement);
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("light");
  });

  it("writes and clears the custom palette variables", () => {
    applyResolvedTheme(
      "custom",
      { primary: "#112233", background: "#445566", darkPalette: true },
      document.documentElement
    );
    expect(document.documentElement.getAttribute("data-theme")).toBe("custom");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("#112233");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-background")).toBe("#445566");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-mode")).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
    applyResolvedTheme("light", undefined, document.documentElement);
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-background")).toBe("");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-mode")).toBe("");
  });

  it("treats a light custom base as a light scheme", () => {
    applyResolvedTheme(
      "custom",
      { primary: "#112233", background: "#445566", darkPalette: false },
      document.documentElement
    );
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-mode")).toBe("light");
  });

  it("clears custom variables when custom carries no palette", () => {
    document.documentElement.style.setProperty("--theme-custom-primary", "#112233");
    applyResolvedTheme("custom", undefined, document.documentElement);
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("");
  });

  it("reads the OS scheme defensively", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(prefersDarkScheme()).toBe(false);
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    expect(prefersDarkScheme()).toBe(true);
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    expect(prefersDarkScheme()).toBe(false);
  });
});
