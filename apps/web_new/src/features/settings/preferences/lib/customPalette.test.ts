// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-theme defaults (parity/specs/profile.md §7, PROF-016).
import { describe, expect, it } from "vitest";

import { customPaletteOrDefault, DEFAULT_CUSTOM_PALETTE } from "./customPalette.js";

describe("customPaletteOrDefault", () => {
  it("keeps a saved palette", () => {
    const saved = { primary: "#102030", background: "#405060", darkPalette: true };
    expect(customPaletteOrDefault(saved)).toBe(saved);
  });

  it("falls back to the usable light default", () => {
    expect(customPaletteOrDefault(undefined)).toEqual({
      primary: "#5e6ad2",
      background: "#ffffff",
      darkPalette: false,
    });
    expect(customPaletteOrDefault(null)).toBe(DEFAULT_CUSTOM_PALETTE);
  });
});
