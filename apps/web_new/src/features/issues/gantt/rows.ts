// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline rows (parity/specs/issues.md §6, ISS-044/049). Every listed
// work item becomes one sidebar row; rows whose dates are set also get a
// bar. A half-dated row keeps its bar (the later chrome slice fades it),
// and an undated row keeps its row so the add-block affordance can live
// there. Bars are day-snapped: both ends cover their whole column.

import type { IssueListItem } from "@pidash/api-client";

import { isTempRowId } from "../freshness/fresh.js";

import { DAY_WIDTHS, addDays, dayOffset, diffDays, isGanttZoom, type GanttRange, type GanttZoom } from "./scale.js";

/** Height of one sidebar row and the chart lane that lines up with it. */
export const ROW_HEIGHT = 44;

/** One timeline row, in list order. */
export interface GanttRow {
  id: string;
  /** Short key shown ahead of the name, e.g. "WEB-42". */
  identifier: string;
  name: string;
  startIso: string | null;
  targetIso: string | null;
  /** Inclusive day count; null until both dates are set. */
  durationDays: number | null;
  stateId: string | null;
  /** Manual rank behind sidebar reorder (ISS-050). */
  sortOrder: number;
  archived: boolean;
  /**
   * An optimistic create that the server has not confirmed yet (ISS-006).
   * Such a row has no server identity, so its bar is inert — no link,
   * peek, reorder or drag — but a quick-add that seeds dates still draws
   * that bar immediately (ISS-055).
   */
  unconfirmed: boolean;
}

/** A bar's box in the timeline strip. */
export interface GanttBarBox {
  left: number;
  width: number;
}

/** Days a bar keeps when only one of its dates is set. */
export const PARTIAL_BAR_DAYS = 2;

function dateOf(value: string | null | undefined): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/** Rows for one project's ordered issue list. */
export function buildRows(issues: readonly IssueListItem[], projectIdentifier: string): GanttRow[] {
  return issues.map((issue) => {
    const startIso = dateOf(issue.start_date);
    const targetIso = dateOf(issue.target_date);
    return {
      id: issue.id,
      identifier: `${projectIdentifier}-${issue.sequence_id}`,
      name: issue.name,
      startIso,
      targetIso,
      durationDays: startIso !== null && targetIso !== null ? diffDays(targetIso, startIso) + 1 : null,
      stateId: issue.state_id,
      sortOrder: issue.sort_order,
      archived: issue.archived_at !== null,
      unconfirmed: isTempRowId(issue.id),
    };
  });
}

/**
 * Whether a row should draw a bar at all. A confirmed row draws when
 * either date exists; an unconfirmed quick-add draws as soon as its
 * seeded dates arrive, so the new bar appears before the server confirms
 * it (ISS-055). Rows without dates draw nothing.
 */
export function hasBar(row: GanttRow): boolean {
  return row.startIso !== null || row.targetIso !== null;
}

/** Bar left/width for a row, or null for an undated row. */
export function barBox(row: GanttRow, range: GanttRange, zoom: GanttZoom): GanttBarBox | null {
  const dayWidth = DAY_WIDTHS[zoom];
  if (row.startIso !== null && row.targetIso !== null) {
    const days = diffDays(row.targetIso, row.startIso) + 1;
    return { left: dayOffset(row.startIso, range, zoom), width: days * dayWidth };
  }
  if (row.startIso !== null) {
    return { left: dayOffset(row.startIso, range, zoom), width: PARTIAL_BAR_DAYS * dayWidth };
  }
  if (row.targetIso !== null) {
    return {
      left: dayOffset(addDays(row.targetIso, -(PARTIAL_BAR_DAYS - 1)), range, zoom),
      width: PARTIAL_BAR_DAYS * dayWidth,
    };
  }
  return null;
}

/** Box for a window whose both ends are known (a live drag's window). */
export function datedBarBox(startIso: string, targetIso: string, range: GanttRange, zoom: GanttZoom): GanttBarBox {
  const dayWidth = DAY_WIDTHS[zoom];
  const days = diffDays(targetIso, startIso) + 1;
  return { left: dayOffset(startIso, range, zoom), width: days * dayWidth };
}

/** Whether a bar lies entirely left or right of the visible strip. */
export function barInView(box: GanttBarBox, scrollLeft: number, viewportWidth: number): boolean {
  return box.left + box.width > scrollLeft && box.left < scrollLeft + viewportWidth;
}

/** Duration copy for a fully dated row, e.g. "5 days"/"1 day". */
export function durationLabel(row: GanttRow): string | null {
  if (row.durationDays === null) return null;
  return `${row.durationDays} ${row.durationDays === 1 ? "day" : "days"}`;
}

/** Zoom from a stored view value, falling back to Week (ISS-045). */
export function parseZoom(value: unknown): GanttZoom {
  return isGanttZoom(value) ? value : "week";
}

/** Height of the chart's two-tier header band (rows start below it). */
export const HEADER_HEIGHT = 48;

/** A rendered slice of the row list (virtualization window). */
export interface RowWindow {
  start: number;
  end: number;
}

/** Ids rendered past each edge of the visible row window. */
export const ROW_OVERSCAN = 6;

/**
 * Rows to mount for a vertical offset and viewport height (ISS-049).
 * Both the sidebar and the lanes use this so they always render the same
 * rows; the scroll height stays the full list because callers reserve
 * `rowCount * rowHeight`.
 */
export function rowWindow(scrollTop: number, viewportHeight: number, rowCount: number, rowHeight: number): RowWindow {
  if (rowCount === 0) return { start: 0, end: 0 };
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - ROW_OVERSCAN);
  const visible = Math.ceil(Math.max(viewportHeight, rowHeight) / rowHeight);
  const end = Math.min(rowCount, first + visible + ROW_OVERSCAN * 2);
  return { start: first, end };
}
