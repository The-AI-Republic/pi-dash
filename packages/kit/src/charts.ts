// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chart colors for @pidash/kit (parity/specs/analytics.md §10,
// NEWFRONT-732). SVG chart libraries take concrete color strings,
// not CSS variables, so this module mirrors the --chart-N series
// plus the accent/grid/tick tokens from tokens.css; charts.test.ts
// pins the mirror against the shipped stylesheet so the CSS stays
// the single source. Light categorical slots are oracle-pinned;
// dark slots are brighter twins for dark surfaces.

/** Resolved chart scheme: contrast and custom themes map onto these. */
export type ChartScheme = "light" | "dark";

/**
 * Ten categorical slots, light theme, in schema order (oracle-pinned).
 * Chart wrappers fill stacked segments from these in order; the
 * shared chart layer extends beyond ten with generated hues.
 */
export const CHART_CATEGORICAL_LIGHT: readonly string[] = [
  "#6172e8",
  "#8b6edb",
  "#e05f99",
  "#29a383",
  "#cb8a37",
  "#3aa7c1",
  "#f1b24a",
  "#e84855",
  "#50c799",
  "#b35f9e",
];

/** Ten categorical slots, dark theme, in schema order. */
export const CHART_CATEGORICAL_DARK: readonly string[] = [
  "#93a0f2",
  "#b094e8",
  "#ec8ab8",
  "#4fc3a2",
  "#dfa761",
  "#66bfd4",
  "#f6c573",
  "#f0707a",
  "#76d8b0",
  "#c984b8",
];

/** Concrete chart chrome per scheme. */
export interface ChartSchemeColors {
  /** Series line/fill for single-series charts (radar). Mirrors --accent. */
  accent: string;
  /** Grid strokes. Mirrors --border. */
  grid: string;
  /** Axis tick labels. Mirrors --text-muted. */
  tick: string;
  /** Categorical slots for this scheme. Mirrors --chart-1..10. */
  categorical: readonly string[];
}

const LIGHT_COLORS: ChartSchemeColors = {
  accent: "#5e6ad2",
  grid: "#e4e4e7",
  tick: "#71717a",
  categorical: CHART_CATEGORICAL_LIGHT,
};

const DARK_COLORS: ChartSchemeColors = {
  accent: "#7c85e0",
  grid: "#26262a",
  tick: "#8b8b93",
  categorical: CHART_CATEGORICAL_DARK,
};

/** Concrete colors for one scheme; dark covers dark and dark-contrast. */
export function chartSchemeColors(scheme: ChartScheme): ChartSchemeColors {
  return scheme === "dark" ? DARK_COLORS : LIGHT_COLORS;
}
