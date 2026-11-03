// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label color palette (parity/specs/issues.md §19, ISS-226): the ten
// preset swatches the inline form offers. Fresh forms seed one of them
// at random; the picker reuses the same seed for inline creates.

/** The ten preset label colors, in swatch order. */
export const LABEL_PALETTE = [
  "#FF6900",
  "#FCB900",
  "#7BDCB5",
  "#00D084",
  "#8ED1FC",
  "#0693E3",
  "#ABB8C3",
  "#EB144C",
  "#F78DA7",
  "#9900EF",
] as const;

/** One preset swatch. */
export type LabelPaletteColor = (typeof LABEL_PALETTE)[number];

/** Seed a fresh label form with a random preset swatch; the random source injects for tests. */
export function randomLabelColor(random: () => number = Math.random): string {
  const index = Math.floor(random() * LABEL_PALETTE.length);
  return LABEL_PALETTE[index] ?? "#FF6900";
}
