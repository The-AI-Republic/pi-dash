// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline model (parity/specs/modules.md §6, MOD-021). Pure: the
// module-to-chart row adapter behind the timeline layout, the status bar
// colors, the bar tooltip text, the manual-rank normalization a reorder
// writes, and the optimistic list-cache paints the timeline mutations
// apply. The chart shell and every gesture stay in features/issues/gantt
// (NEWFRONT-624); nothing here touches React, the router or the network.

import type { ModuleRow } from "@pidash/api-client";

import { diffDays, RANK_STEP, type GanttRow } from "../../../issues/index.js";
import { formatModuleDay, parseModuleDay } from "../../lib/moduleDates.js";
import { moduleStatusColor, type ModuleSortKey } from "../../lib/moduleListControls.js";

/** A module's plot window: both ends present, or no bar at all (§6). */
export interface ModuleTimelineRange {
  startIso: string;
  targetIso: string;
}

function isoDay(value: string | null): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/**
 * The range a module plots, or null when either end is missing (§6: a
 * module without a complete date range renders its row but no bar).
 */
export function moduleTimelineRange(module: Pick<ModuleRow, "start_date" | "target_date">): ModuleTimelineRange | null {
  const startIso = isoDay(module.start_date);
  const targetIso = isoDay(module.target_date);
  if (startIso === null || targetIso === null) return null;
  return { startIso, targetIso };
}

/**
 * Chart rows for the ordered module list. Only complete ranges plot; a
 * half-dated or undated module maps to a row with no dates so the lane
 * keeps its no-dates reading. Modules have no short key, no workflow
 * state and no optimistic creates: those lanes stay at their defaults and
 * the entity vocabulary (row prefix, href, duration) renders the rest.
 */
export function buildModuleTimelineRows(modules: readonly ModuleRow[]): GanttRow[] {
  return modules.map((module) => {
    const range = moduleTimelineRange(module);
    return {
      id: module.id,
      identifier: "",
      name: module.name,
      startIso: range?.startIso ?? null,
      targetIso: range?.targetIso ?? null,
      durationDays: range === null ? null : diffDays(range.targetIso, range.startIso) + 1,
      stateId: null,
      sortOrder: module.sort_order,
      priority: "none",
      sequenceId: 0,
      archived: (module.archived_at ?? null) !== null,
      unconfirmed: false,
      isEpic: false,
    };
  });
}

/** Bar colors by module id: every module plots in its status color (§4.2). */
export function moduleTimelineBarColors(modules: readonly ModuleRow[]): Record<string, string> {
  const colors: Record<string, string> = {};
  for (const module of modules) {
    colors[module.id] = moduleStatusColor(module.status);
  }
  return colors;
}

/** One tooltip day; an unparseable stored value reads back verbatim. */
export function moduleTimelineTooltipDay(iso: string): string {
  const parsed = parseModuleDay(iso);
  return parsed === null ? iso : formatModuleDay(parsed);
}

/**
 * The bar tooltip: the module name plus its date summary (§6). Null for a
 * module without a complete range, which renders no bar to hover.
 */
export function moduleTimelineTooltip(module: Pick<ModuleRow, "name" | "start_date" | "target_date">): string | null {
  const range = moduleTimelineRange(module);
  if (range === null) return null;
  return `${module.name}: ${moduleTimelineTooltipDay(range.startIso)} to ${moduleTimelineTooltipDay(range.targetIso)}`;
}

/**
 * The tooltip for one chart row, or null when the row's module is unknown
 * or dateless. The chart calls back with its own rows; this keeps the
 * component's lookup total so it never branches on a missing module.
 */
export function moduleTimelineTooltipForId(modules: ReadonlyMap<string, ModuleRow>, rowId: string): string | null {
  const module = modules.get(rowId);
  if (module === undefined) return null;
  return moduleTimelineTooltip(module);
}

/**
 * The bar tooltip for one chart row: the name plus its date summary, or
 * the bare name when the row's module is unknown or dateless (bars only
 * render for dated modules, so the fallback never shows on a bar).
 */
export function moduleTimelineBarTitle(
  modules: ReadonlyMap<string, ModuleRow>,
  row: Pick<GanttRow, "id" | "name">
): string {
  return moduleTimelineTooltipForId(modules, row.id) ?? row.name;
}

/** The status the sidebar glyph plots; an unknown row reads as Backlog. */
export function moduleTimelinePrefixStatus(
  modules: ReadonlyMap<string, ModuleRow>,
  rowId: string
): ModuleRow["status"] {
  return modules.get(rowId)?.status ?? "backlog";
}

/**
 * Normalized manual ranks for a committed row order: distinct and ascending
 * in the new sequence, on the chart engine's rank step. Fresh modules share
 * the default rank, so the moved row alone cannot carry the order — every
 * visible row takes its position.
 */
export function normalizeModuleTimelineRanks(order: readonly string[]): Map<string, number> {
  const ranks = new Map<string, number>();
  for (const [index, id] of order.entries()) {
    ranks.set(id, (index + 1) * RANK_STEP);
  }
  return ranks;
}

/** One rank write: the module whose stored rank must move. */
export interface ModuleTimelineRankWrite {
  id: string;
  sort_order: number;
}

/** The rows whose stored rank differs from the normalized one. */
export function changedModuleTimelineRanks(
  modules: readonly Pick<ModuleRow, "id" | "sort_order">[],
  ranks: ReadonlyMap<string, number>
): ModuleTimelineRankWrite[] {
  const writes: ModuleTimelineRankWrite[] = [];
  for (const module of modules) {
    const rank = ranks.get(module.id);
    if (rank !== undefined && rank !== module.sort_order) {
      writes.push({ id: module.id, sort_order: rank });
    }
  }
  return writes;
}

function isModuleRowArray(data: unknown): data is ModuleRow[] {
  return Array.isArray(data);
}

/**
 * Paint one module's dates onto a cached list (the bare-array list read).
 * Non-list cache entries pass through untouched.
 */
export function withModuleTimelineDates(
  data: unknown,
  moduleId: string,
  dates: { startDate: string | null; targetDate: string | null }
): unknown {
  if (!isModuleRowArray(data)) return data;
  return data.map((module) =>
    module.id === moduleId ? { ...module, start_date: dates.startDate, target_date: dates.targetDate } : module
  );
}

/**
 * Paint normalized ranks onto a cached list after a reorder. The manual
 * sort reads these ranks, so the new order shows before the writes land.
 * Non-list cache entries pass through untouched.
 */
export function withModuleTimelineRanks(data: unknown, writes: readonly ModuleTimelineRankWrite[]): unknown {
  if (!isModuleRowArray(data)) return data;
  const ranks = new Map(writes.map((write) => [write.id, write.sort_order]));
  return data.map((module) => {
    const rank = ranks.get(module.id);
    return rank === undefined ? module : { ...module, sort_order: rank };
  });
}

/** Warning when a reorder is attempted outside manual placement (§6.3). */
export const MODULE_TIMELINE_REORDER_WARNING_TITLE = "Manual placement required";
export const MODULE_TIMELINE_REORDER_WARNING_BODY = "Change the ordering to Manual to reorder rows.";

/**
 * Whether a travelled row press warns. Editors outside manual placement
 * learn the gate; guests stay silent (§6: a guest attempt is a no-op with
 * no toast). Under manual placement with edit standing the reorder binding
 * is mounted, so the attempt callback never fires.
 */
export function warnOnTimelineReorderAttempt(canEdit: boolean, sortKey: ModuleSortKey): boolean {
  return canEdit && sortKey !== "manual";
}

/** Notice when the server refuses a timeline reorder. */
export const MODULE_TIMELINE_REORDER_FAILED_TITLE = "Couldn't save the module order";
export const MODULE_TIMELINE_REORDER_FAILED_BODY =
  "The module order is unchanged — check your connection and try again.";
