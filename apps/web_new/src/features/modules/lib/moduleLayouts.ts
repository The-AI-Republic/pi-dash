// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules list layouts (parity/specs/modules.md §3.4, MOD-011). Pure: the
// three layout keys, their display order, the labels the switcher and the
// narrow-screen menu render, and the parse fallback. Modules have their own
// vocabulary — a board is a gallery and the chart is a timeline — so these
// keys are deliberately not the Issues layout keys (`kanban`,
// `gantt_chart`); the two surfaces persist separately (§13.3.1).

/** The three modules-list layouts, in switcher order. */
export const MODULES_LAYOUTS = ["list", "board", "timeline"] as const;
export type ModulesLayout = (typeof MODULES_LAYOUTS)[number];

/** Fallback when nothing is stored or the stored value is unknown. */
export const DEFAULT_MODULES_LAYOUT: ModulesLayout = "list";

/** Wide-container switcher labels, in canonical order. */
export const MODULES_LAYOUT_LABELS: Record<ModulesLayout, string> = {
  list: "List",
  board: "Board",
  timeline: "Timeline",
};

/**
 * Narrow-screen menu entries (§3.4): exactly two, list and board, with the
 * pinned labels the oracle reads. Timeline is not offered from the list on
 * small screens.
 */
export const MODULES_COMPACT_LAYOUT_OPTIONS: readonly { layout: "list" | "board"; label: string }[] = [
  { layout: "list", label: "List layout" },
  { layout: "board", label: "Gallery layout" },
];

/** The narrow-screen choice, never timeline. */
export type ModulesCompactLayout = (typeof MODULES_COMPACT_LAYOUT_OPTIONS)[number]["layout"];

export function isModulesLayout(value: unknown): value is ModulesLayout {
  return value === "list" || value === "board" || value === "timeline";
}

/** Read a stored layout value, falling back to the list layout. */
export function parseModulesLayout(value: unknown): ModulesLayout {
  return isModulesLayout(value) ? value : DEFAULT_MODULES_LAYOUT;
}

/**
 * The layout a narrow screen may actually render. A project remembered on
 * timeline reopens as its closest offered layout (list) on a small screen,
 * without overwriting the stored choice for wide screens.
 */
export function resolveCompactLayout(layout: ModulesLayout): ModulesCompactLayout {
  return layout === "board" ? "board" : "list";
}
