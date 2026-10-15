// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin theme without a DOM: storage and document access degrade to
// no-ops instead of throwing.
import { describe, expect, it } from "vitest";

import { applyAdminThemePreference, applyStoredAdminTheme, readStoredAdminTheme, useAdminThemeStore } from "./theme.js";

describe("admin theme without a DOM", () => {
  it("falls back to system with no store to read", () => {
    expect(readStoredAdminTheme()).toBe("system");
  });

  it("applies nothing with no document to mark", () => {
    expect(() => applyAdminThemePreference("dark")).not.toThrow();
    expect(() => applyStoredAdminTheme()).not.toThrow();
  });

  it("keeps the mode in memory when nothing persists it", () => {
    useAdminThemeStore.getState().setMode("dark");
    expect(useAdminThemeStore.getState().mode).toBe("dark");
    useAdminThemeStore.getState().setMode("system");
    expect(useAdminThemeStore.getState().mode).toBe("system");
  });
});
