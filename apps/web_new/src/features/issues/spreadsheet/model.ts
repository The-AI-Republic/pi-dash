// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spreadsheet column model (parity/specs/issues.md §3, ISS-015/016/018).
// Pure helpers: the fixed column order, display-property and project
// feature gating, sort menu metadata, and the persisted local sort key
// the header marker reads. The table renders what these functions
// select; cells own their own writers.

import { isPropertyShown, visiblePropertyKeys, type DisplayFeatureFlags } from "../display/matrix.js";
import type { IssueListItem } from "@pidash/api-client";
import { isCompletedCycle } from "../../../shared/pickers/cycles.js";

/** One spreadsheet property column, keyed by display-property name. */
export interface SheetColumn {
  key: string;
  label: string;
  /** Column header sort metadata; absent means not sortable. */
  sort?: SheetSort;
}

export interface SheetSort {
  /** order_by key that sorts ascending by this column. */
  ascending: string;
  /** order_by key that sorts descending by this column. */
  descending: string;
}

/** Header labels in render order (spec §3, ISS-016). */
export const SHEET_COLUMNS: readonly SheetColumn[] = [
  { key: "state", label: "State", sort: { ascending: "state__name", descending: "-state__name" } },
  {
    key: "priority",
    label: "Priority",
    sort: { ascending: "priority", descending: "-priority" },
  },
  {
    key: "assignee",
    label: "Assignees",
    sort: { ascending: "assignees__first_name", descending: "-assignees__first_name" },
  },
  {
    key: "labels",
    label: "Labels",
    sort: { ascending: "labels__name", descending: "-labels__name" },
  },
  {
    key: "modules",
    label: "Modules",
    sort: { ascending: "issue_module__module__name", descending: "-issue_module__module__name" },
  },
  {
    key: "cycle",
    label: "Cycle",
    sort: { ascending: "issue_cycle__cycle__name", descending: "-issue_cycle__cycle__name" },
  },
  {
    key: "start_date",
    label: "Start date",
    sort: { ascending: "-start_date", descending: "start_date" },
  },
  {
    key: "due_date",
    label: "Due date",
    sort: { ascending: "-target_date", descending: "target_date" },
  },
  {
    key: "estimate",
    label: "Estimate",
    sort: { ascending: "estimate_point__key", descending: "-estimate_point__key" },
  },
  {
    key: "created_on",
    label: "Created on",
    sort: { ascending: "-created_at", descending: "created_at" },
  },
  {
    key: "updated_on",
    label: "Updated on",
    sort: { ascending: "-updated_at", descending: "updated_at" },
  },
  {
    key: "link",
    label: "Link",
    sort: { ascending: "-link_count", descending: "link_count" },
  },
  {
    key: "attachment_count",
    label: "Attachment",
    sort: { ascending: "-attachment_count", descending: "attachment_count" },
  },
  {
    key: "sub_issue_count",
    label: "Sub-work item",
    sort: { ascending: "-sub_issues_count", descending: "sub_issues_count" },
  },
];

/** Sort menu entries, in ascending/descending order. */
export const SHEET_SORT_ASCENDING_LABEL = "Ascending";
export const SHEET_SORT_DESCENDING_LABEL = "Descending";
export const SHEET_SORT_CLEAR_LABEL = "Clear sorting";

/** The order a cleared sort restores (spec §3, ISS-018). */
export const SHEET_DEFAULT_ORDER = "-created_at";

/** Storage key for the local header-sort pair (spec §3, ISS-018). */
export const SHEET_SORT_STORAGE_KEY = "web_new.spreadsheet.sort";

/** Reading direction of the persisted sort pair. */
export type SheetSortDirection = "ascending" | "descending";

export interface SheetSortState {
  column: string;
  direction: SheetSortDirection;
  orderBy: string;
}

/** The sort state a column reports from the stored orderBy. */
export function sheetSortState(column: SheetColumn, orderBy: string): SheetSortState | null {
  if (column.sort === undefined) return null;
  if (orderBy === column.sort.ascending) {
    return { column: column.key, direction: "ascending", orderBy };
  }
  if (orderBy === column.sort.descending) {
    return { column: column.key, direction: "descending", orderBy };
  }
  return null;
}

/**
 * The active sort state across the whole column list. The default
 * creation order is not a chosen sort, so it carries no marker.
 */
export function activeSheetSort(columns: readonly SheetColumn[], orderBy: string): SheetSortState | null {
  if (orderBy === SHEET_DEFAULT_ORDER) return null;
  for (const column of columns) {
    const state = sheetSortState(column, orderBy);
    if (state !== null) return state;
  }
  return null;
}

/**
 * The sort a header should mark. The locally remembered pair wins while
 * it still matches the server order, because the Created-on ascending
 * sort shares `-created_at` with the unsorted default. A stale pair is
 * ignored and the server order decides.
 */
export function resolveSheetSort(
  columns: readonly SheetColumn[],
  orderBy: string,
  stored: SheetSortState | null
): SheetSortState | null {
  if (stored !== null && stored.orderBy === orderBy) {
    const column = columns.find((entry) => entry.key === stored.column);
    const fromServer = column === undefined ? null : sheetSortState(column, orderBy);
    if (fromServer !== null && fromServer.direction === stored.direction) return fromServer;
  }
  return activeSheetSort(columns, orderBy);
}

/** Rows in a completed cycle never accept inline edits (ISS-017). */
export function isSheetRowReadOnly(
  issue: Pick<IssueListItem, "archived_at" | "cycle_id">,
  cycles: readonly { id: string; status?: string | undefined }[]
): boolean {
  if (issue.archived_at !== null) return true;
  if (issue.cycle_id === null) return false;
  return isCompletedCycle(cycles.find((cycle) => cycle.id === issue.cycle_id)?.status);
}

/** The order_by a menu pick writes. */
export function sheetSortOrder(column: SheetColumn, direction: SheetSortDirection, currentOrder: string): string {
  if (column.sort === undefined) return currentOrder;
  return direction === "ascending" ? column.sort.ascending : column.sort.descending;
}

/**
 * The property columns the sheet renders: display properties that are
 * enabled, minus cycle/module while their project features are off
 * (ISS-016). Estimate also needs an active system, which the caller
 * gates separately; the flags here cover the project switches.
 */
export function visibleSheetColumns(properties: Record<string, boolean>, flags: DisplayFeatureFlags): SheetColumn[] {
  return visiblePropertyKeys(
    SHEET_COLUMNS.map((column) => column.key),
    flags
  )
    .filter((key) => isPropertyShown(properties, key))
    .map((key) => SHEET_COLUMNS.find((column) => column.key === key))
    .filter((column): column is SheetColumn => column !== undefined);
}

/** Whether a property key maps to a known spreadsheet column. */
export function isSheetColumnKey(key: string): boolean {
  return SHEET_COLUMNS.some((column) => column.key === key);
}

/** Safe parse of the persisted local sort pair. */
export function parseStoredSheetSort(raw: string | null): SheetSortState | null {
  if (raw === null || raw.trim() === "") return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null) return null;
    const record = value as Record<string, unknown>;
    const column = record["column"];
    const direction = record["direction"];
    const orderBy = record["orderBy"];
    if (typeof column !== "string" || typeof orderBy !== "string") return null;
    if (direction !== "ascending" && direction !== "descending") return null;
    return { column, direction, orderBy };
  } catch {
    return null;
  }
}

/** Serialize the local sort pair for storage. */
export function serializeSheetSort(state: SheetSortState | null): string {
  return state === null ? "" : JSON.stringify(state);
}

/** Cell grid coordinates used by keyboard navigation (ISS-020). */
export interface SheetCell {
  row: number;
  column: number;
}

/**
 * Move focus one cell in an arrow direction. The grid clamps at its
 * edges instead of wrapping, so the focused cell never disappears.
 */
export function moveSheetFocus(
  cell: SheetCell,
  direction: "up" | "down" | "left" | "right",
  rows: number,
  columns: number
): SheetCell {
  if (rows <= 0 || columns <= 0) return cell;
  const row = Math.min(rows - 1, Math.max(0, cell.row));
  const column = Math.min(columns - 1, Math.max(0, cell.column));
  switch (direction) {
    case "up":
      return { row: Math.max(0, row - 1), column };
    case "down":
      return { row: Math.min(rows - 1, row + 1), column };
    case "left":
      return { row, column: Math.max(0, column - 1) };
    case "right":
      return { row, column: Math.min(columns - 1, column + 1) };
  }
}

/** DOM id for one cell, so a focus move can find it by coordinates. */
export function sheetCellId(rowId: string, columnKey: string): string {
  return `sheet-cell-${rowId}-${columnKey}`;
}

/** DOM id for the row's leading work-item cell. */
export function sheetFirstCellId(rowId: string): string {
  return `issue-${rowId}`;
}
