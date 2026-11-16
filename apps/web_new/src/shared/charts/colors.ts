// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart color engine (parity/specs/analytics.md §10,
// NEWFRONT-732). Resolves categorical segment colors from the kit
// chart palette: the first ten slots come from @pidash/kit tokens,
// further slots extend deterministically with generated hues rather
// than cycling and duplicating colors. No palette is hard-coded
// here; the kit tokens stay the single source.

import { CHART_CATEGORICAL_DARK, CHART_CATEGORICAL_LIGHT, type ChartScheme } from "@pidash/kit";

import type { ResolvedScheme } from "../../core/theme/index.js";

/** Slots the kit palette carries per scheme. */
export const CHART_PALETTE_SLOTS = 10;

/** Golden-angle step between generated hues (degrees). */
const GENERATED_HUE_STEP = 137.508;

/** Anchor hue for the generated extension (indigo family). */
const GENERATED_HUE_BASE = 222;

/** Saturation/lightness of generated hues per scheme. */
const GENERATED_TONE: Record<ChartScheme, { saturation: number; lightness: number }> = {
  light: { saturation: 65, lightness: 55 },
  dark: { saturation: 70, lightness: 68 },
};

/**
 * Map a resolved document scheme onto a chart scheme. Dark and
 * dark-contrast render dark charts; a custom theme follows its
 * palette's darkness; everything else renders light charts.
 */
export function resolveChartScheme(scheme: ResolvedScheme, customDark: boolean): ChartScheme {
  if (scheme === "dark" || scheme === "dark-contrast") return "dark";
  if (scheme === "custom") return customDark ? "dark" : "light";
  return "light";
}

function hslToHex(hue: number, saturation: number, lightness: number): string {
  const s = saturation / 100;
  const l = lightness / 100;
  const k = (n: number): number => (n + hue / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const channel = (n: number): number => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const hex = (value: number): string =>
    Math.round(value * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${hex(channel(0))}${hex(channel(8))}${hex(channel(4))}`;
}

/** Generated hue for the nth slot past the kit palette (n >= 0). */
function generatedSlotColor(n: number, scheme: ChartScheme): string {
  const tone = GENERATED_TONE[scheme];
  const hue = (GENERATED_HUE_BASE + n * GENERATED_HUE_STEP) % 360;
  return hslToHex(hue, tone.saturation, tone.lightness);
}

/**
 * Color for one categorical slot in schema order. Slots below ten
 * read the kit palette; slots past ten extend with generated hues.
 * Negative indices clamp to the first slot.
 */
export function chartSlotColor(index: number, scheme: ChartScheme): string {
  const slot = Math.max(Math.floor(index), 0);
  if (slot < CHART_PALETTE_SLOTS) {
    const palette = scheme === "dark" ? CHART_CATEGORICAL_DARK : CHART_CATEGORICAL_LIGHT;
    return palette[slot]!;
  }
  return generatedSlotColor(slot - CHART_PALETTE_SLOTS, scheme);
}

/**
 * First `count` categorical colors in schema order. Empty for
 * non-positive counts.
 */
export function chartPalette(count: number, scheme: ChartScheme): string[] {
  if (count <= 0) return [];
  return Array.from({ length: Math.floor(count) }, (_, index) => chartSlotColor(index, scheme));
}
