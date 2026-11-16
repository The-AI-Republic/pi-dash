// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue layout kinds (parity/specs/issues.md §1, ISS-001). The five
// layouts every issues-bearing entity can render, in switcher order.
// Layout slices register their renderer under one of these keys; the
// switcher offers exactly the registered set, so it grows without
// rework as layouts land.

/** The five issue layouts, keyed as the server stores them. */
export type LayoutKind = "list" | "kanban" | "calendar" | "spreadsheet" | "gantt_chart";

/** Switcher order: list, board, calendar, spreadsheet, timeline. */
export const LAYOUT_ORDER: readonly LayoutKind[] = ["list", "kanban", "calendar", "spreadsheet", "gantt_chart"];

/** The compact header offers list, board and calendar only (ISS-003). */
export const MOBILE_LAYOUT_ORDER: readonly LayoutKind[] = ["list", "kanban", "calendar"];

/** Human labels for the dropdown and mobile menu entries. */
export const LAYOUT_LABELS: Record<LayoutKind, string> = {
  list: "List",
  kanban: "Board",
  calendar: "Calendar",
  spreadsheet: "Spreadsheet",
  gantt_chart: "Timeline",
};

/** Fallback when nothing is stored or the stored value is unknown. */
export const DEFAULT_LAYOUT: LayoutKind = "list";

export function isLayoutKind(value: unknown): value is LayoutKind {
  return (
    value === "list" || value === "kanban" || value === "calendar" || value === "spreadsheet" || value === "gantt_chart"
  );
}

/** Read a stored layout value, falling back to the list layout. */
export function parseLayoutKind(value: unknown): LayoutKind {
  return isLayoutKind(value) ? value : DEFAULT_LAYOUT;
}

/** The mobile menu offers the registered layouts in mobile order. */
export function mobileOfferedLayouts(offered: readonly LayoutKind[]): LayoutKind[] {
  return MOBILE_LAYOUT_ORDER.filter((kind) => offered.includes(kind));
}

/**
 * The layout to render: the stored choice when it is offered, else the
 * first offered layout. Archived contexts offer only list, so they
 * always resolve to list regardless of the stored choice (ISS-001).
 */
export function resolveActiveLayout(preferred: LayoutKind, offered: readonly LayoutKind[]): LayoutKind {
  if (offered.includes(preferred)) return preferred;
  return offered[0] ?? DEFAULT_LAYOUT;
}
