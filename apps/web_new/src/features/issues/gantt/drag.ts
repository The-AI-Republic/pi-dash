// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline gesture math (parity/specs/issues.md §6, ISS-050–055). Pure
// helpers behind the sidebar reorder, the bar move and resize, and the
// add-block planting: day snapping from a pixel delta, neighbour-derived
// sort ranks, inclusive one-day minimums, and the plant window per zoom.
// The components own pointer state and persistence; nothing here touches
// React or the network.

import { addDays, diffDays, type GanttRange, type GanttZoom } from "./scale.js";

/** Pixel travel that still counts as a click, not a drag. */
export const DRAG_THRESHOLD_PX = 4;

/** A row's identity and manual rank, as the sidebar sees it. */
export interface ReorderEntry {
  id: string;
  sortOrder: number;
}

/** Where a dragged row lands relative to its target row. */
export type DropPlacement = "before" | "after";

/** A committed reorder: the new id order and the moved row's rank. */
export interface ReorderResult {
  order: string[];
  sortOrder: number;
}

/** Rank step used when a row lands at either end of the list. */
export const RANK_STEP = 1000;

/**
 * Insert `sourceId` next to `targetId` and derive the moved row's new
 * manual rank from its neighbours (ISS-050): a step below the first row,
 * a step above the last, and the midpoint between two neighbours. Returns
 * null for an unknown id, a drop onto itself, or a list too short to
 * reorder.
 */
export function deriveReorder(
  entries: readonly ReorderEntry[],
  sourceId: string,
  targetId: string,
  placement: DropPlacement = "before"
): ReorderResult | null {
  if (sourceId === targetId) return null;
  const ids = entries.map((entry) => entry.id);
  const sourceIndex = ids.indexOf(sourceId);
  const targetIndex = ids.indexOf(targetId);
  if (sourceIndex === -1 || targetIndex === -1) return null;

  const order = [...ids];
  order.splice(sourceIndex, 1);
  // The target is still in the list: it differs from the source, and only
  // the source was removed.
  const anchor = order.indexOf(targetId);
  const insertAt = placement === "after" ? anchor + 1 : anchor;
  order.splice(insertAt, 0, sourceId);
  if (order.every((id, index) => id === ids[index])) return null;

  const ranks = new Map(entries.map((entry) => [entry.id, entry.sortOrder]));
  const rankOf = (id: string | undefined): number | undefined => (id === undefined ? undefined : ranks.get(id));
  const previous = rankOf(order[insertAt - 1]);
  const next = rankOf(order[insertAt + 1]);

  // A present neighbour always carries a rank (ids come from entries),
  // and the list always has at least two ids here, so at least one end
  // exists: first row, last row, or the midpoint between two neighbours.
  const sortOrder =
    previous !== undefined && next !== undefined
      ? (previous + next) / 2
      : next !== undefined
        ? next - RANK_STEP
        : (previous as number) + RANK_STEP;

  return { order, sortOrder };
}

/** Whole days covered by a horizontal travel, snapped to the nearest day. */
export function snappedDayDelta(deltaPx: number, dayWidth: number): number {
  if (!Number.isFinite(deltaPx) || dayWidth <= 0) return 0;
  return Math.round(deltaPx / dayWidth);
}

/** Day column under a strip-relative x offset (never negative). */
export function dayIndexAt(x: number, dayWidth: number): number {
  if (!Number.isFinite(x) || dayWidth <= 0) return 0;
  return Math.max(0, Math.floor(x / dayWidth));
}

/** The date window an add-block plants at a day column (ISS-054). */
export interface PlantedDates {
  startDate: string;
  targetDate: string;
}

/**
 * Plant a block starting on the hovered column: one day in Week/Month,
 * a week in Quarter. The target is the inclusive end of the span, which
 * the oracle reads as a one-day (or seven-day) difference (ISS-054).
 */
export function plantedDates(rangeStartIso: string, dayIndex: number, zoom: GanttZoom): PlantedDates {
  const startDate = addDays(rangeStartIso, Math.max(0, Math.trunc(dayIndex)));
  const span = zoom === "quarter" ? 7 : 1;
  return { startDate, targetDate: addDays(startDate, span) };
}

/** A row's dates after a gesture; either end may still be missing. */
export interface GestureDates {
  startDate: string | null;
  targetDate: string | null;
}

/** The base window of a half-dated row: one day around the known end. */
export function baseWindow(row: Pick<GestureDates, "startDate" | "targetDate">): GestureDates | null {
  if (row.startDate !== null && row.targetDate !== null) {
    return { startDate: row.startDate, targetDate: row.targetDate };
  }
  if (row.startDate !== null) return { startDate: row.startDate, targetDate: addDays(row.startDate, 1) };
  if (row.targetDate !== null) return { startDate: addDays(row.targetDate, -1), targetDate: row.targetDate };
  return null;
}

/** Whole-day length of a fully dated window (inclusive). */
export function windowDays(dates: GestureDates): number | null {
  if (dates.startDate === null || dates.targetDate === null) return null;
  return diffDays(dates.targetDate, dates.startDate) + 1;
}

/**
 * Move both ends by the same day offset, preserving the window length
 * (ISS-051). Only fully dated rows move.
 */
export function movedDates(row: GestureDates, dayDelta: number): GestureDates | null {
  if (row.startDate === null || row.targetDate === null) return null;
  return {
    startDate: addDays(row.startDate, dayDelta),
    targetDate: addDays(row.targetDate, dayDelta),
  };
}

/** A resize gesture: the full window to draw, and only the ends to write. */
export interface ResizeResult {
  /** Both ends of the drawn window, filling a missing end for display. */
  window: GestureDates;
  /**
   * The ends the gesture persists. A start-only row dragged by the left
   * handle writes its start and leaves the target absent; a target-only
   * row dragged by the right handle writes its target and leaves the
   * start absent. The non-dragged end is never invented.
   */
  write: GestureDates;
}

/**
 * Resize one end by a day offset (ISS-052/053). The dragged end snaps to
 * whole days and never crosses the fixed end, so the window keeps at
 * least one day. A half-dated row gains the missing end for the preview,
 * and that end is persisted only when it is the one being dragged (the
 * old app's `shouldUpdateHalfBlock` rule).
 */
export function resizedDates(row: GestureDates, side: "left" | "right", dayDelta: number): ResizeResult | null {
  const base = baseWindow(row);
  if (base === null || base.startDate === null || base.targetDate === null) return null;
  if (side === "left") {
    const startDate = addDays(base.startDate, dayDelta);
    const clamped = startDate > base.targetDate ? base.targetDate : startDate;
    return {
      window: { startDate: clamped, targetDate: base.targetDate },
      write: { startDate: clamped, targetDate: row.targetDate },
    };
  }
  const targetDate = addDays(base.targetDate, dayDelta);
  const clamped = targetDate < base.startDate ? base.startDate : targetDate;
  return {
    window: { startDate: base.startDate, targetDate: clamped },
    write: { startDate: row.startDate, targetDate: clamped },
  };
}

/** Date under one resize handle, for the floating preview (ISS-052). */
export function handlePreviewDate(row: GestureDates, side: "left" | "right", dayDelta = 0): string | null {
  const base = baseWindow(row);
  if (base === null) return null;
  // baseWindow always returns both ends, so the dragged one is present.
  return addDays((side === "left" ? base.startDate : base.targetDate) as string, dayDelta);
}

/** Tooltip date for the add-block control at a day column (ISS-054). */
export function addBlockDate(range: Pick<GanttRange, "startIso">, dayIndex: number): string {
  return addDays(range.startIso, Math.max(0, Math.trunc(dayIndex)));
}
