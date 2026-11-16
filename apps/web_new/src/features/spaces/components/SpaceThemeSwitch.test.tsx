// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SPACE_THEME_STORAGE_KEY } from "../lib/spaceTheme.js";

import { SpaceThemeSwitch } from "./SpaceThemeSwitch.js";

function stubColorScheme(dark: boolean): void {
  vi.stubGlobal("matchMedia", () => ({
    matches: dark,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
}

afterEach(() => {
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute("data-theme");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("SpaceThemeSwitch", () => {
  it("offers the dark move while light, and flips on click", () => {
    stubColorScheme(false);
    render(<SpaceThemeSwitch />);
    const control = screen.getByTestId("space-theme-switch");
    expect(control).toHaveAttribute("aria-label", "Switch to dark theme");
    fireEvent.click(control);
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(screen.getByTestId("space-theme-switch")).toHaveAttribute("aria-label", "Switch to light theme");
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBe("dark");
  });

  it("follows a dark OS on first visit", () => {
    stubColorScheme(true);
    render(<SpaceThemeSwitch />);
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(screen.getByTestId("space-theme-switch")).toHaveAttribute("aria-label", "Switch to light theme");
  });
});
