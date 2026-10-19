// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-theme defaults (parity/specs/profile.md §7, PROF-016). A member
// who has never saved a custom palette starts the designer from a usable
// pair instead of black-on-black; saved colors always win over these.

import type { CustomPalette } from "../../../../core/theme/index.js";

export const DEFAULT_CUSTOM_PALETTE: CustomPalette = {
  primary: "#5e6ad2",
  background: "#ffffff",
  darkPalette: false,
};

/** The saved palette, or the defaults while none exists. */
export function customPaletteOrDefault(palette: CustomPalette | undefined | null): CustomPalette {
  if (palette === undefined || palette === null) return DEFAULT_CUSTOM_PALETTE;
  return palette;
}
