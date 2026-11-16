// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list presentations (parity/specs/cycles.md §21, CYC-048).
// Pure: the two row-presentation keys the narrow-screen list menu
// offers, with the pinned labels the oracle reads, and the parse
// fallback. Gallery only re-presents the same rows — same order,
// same hero and groups — so these keys are deliberately not the
// Issues layout keys; nothing here reads or writes the server.

/** The two cycles-list presentations, in menu order. */
export const CYCLE_LIST_LAYOUTS = ["list", "gallery"] as const;
export type CycleListLayout = (typeof CYCLE_LIST_LAYOUTS)[number];

/** Fallback when nothing is stored or the stored value is unknown. */
export const DEFAULT_CYCLE_LIST_LAYOUT: CycleListLayout = "list";

/**
 * Narrow-screen menu entries (§21): exactly two, with the pinned
 * labels the oracle asserts as an array. No third entry exists here;
 * the reference's timeline entry is not offered on narrow screens.
 */
export const CYCLE_LIST_LAYOUT_OPTIONS: readonly { layout: CycleListLayout; label: string }[] = [
  { layout: "list", label: "List layout" },
  { layout: "gallery", label: "Gallery layout" },
];

export function isCycleListLayout(value: unknown): value is CycleListLayout {
  return value === "list" || value === "gallery";
}

/** Read a stored presentation value, falling back to the list rows. */
export function parseCycleListLayout(value: unknown): CycleListLayout {
  return isCycleListLayout(value) ? value : DEFAULT_CYCLE_LIST_LAYOUT;
}

/** The menu label for a presentation key. */
export function cycleListLayoutLabel(layout: CycleListLayout): string {
  return layout === "gallery" ? "Gallery layout" : "List layout";
}
