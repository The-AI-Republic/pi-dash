// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared bar end-rounding rules (parity/specs/analytics.md §10,
// NEWFRONT-732). Single-series bars round both ends; stacked bars
// round only the true extremes — the lowest nonzero segment rounds
// its bottom, the highest nonzero segment rounds its top, interior
// and zero-value segments stay square. Pure: the bar wrapper turns
// these tuples into per-datum cell radii.

/** Corner radii [top-left, top-right, bottom-right, bottom-left]. */
export type BarSegmentRadius = [number, number, number, number];

/** Rounded end radius, matching the kit control radius. */
export const BAR_END_RADIUS = 4;

const SQUARE: BarSegmentRadius = [0, 0, 0, 0];

/** Single-series bars: every bar rounds both ends. */
export function singleBarRadius(radius: number = BAR_END_RADIUS): BarSegmentRadius {
  return [radius, radius, 0, 0];
}

/**
 * Radius for one stacked segment. `values` holds the bar's segment
 * values from bottom to top; the lowest nonzero segment rounds its
 * bottom, the highest nonzero segment rounds its top, and every
 * other segment stays square. A lone nonzero segment rounds both
 * its ends; a bar with no nonzero segment stays fully square.
 */
export function stackedSegmentRadius(
  values: readonly number[],
  segmentIndex: number,
  radius: number = BAR_END_RADIUS
): BarSegmentRadius {
  let lowest = -1;
  let highest = -1;
  for (let index = 0; index < values.length; index += 1) {
    if (values[index] !== 0) {
      if (lowest === -1) lowest = index;
      highest = index;
    }
  }
  if (lowest === -1 || highest === -1) return SQUARE;
  if (segmentIndex === lowest && segmentIndex === highest) return [radius, radius, radius, radius];
  if (segmentIndex === lowest) return [0, 0, radius, radius];
  if (segmentIndex === highest) return [radius, radius, 0, 0];
  return SQUARE;
}

/**
 * Per-bar, per-segment radii for one stacked chart. `values[bar]`
 * holds that bar's segment values from bottom to top; the result
 * mirrors the shape.
 */
export function stackedBarRadii(
  values: readonly (readonly number[])[],
  radius: number = BAR_END_RADIUS
): BarSegmentRadius[][] {
  return values.map((bar) => bar.map((_, index) => stackedSegmentRadius(bar, index, radius)));
}
