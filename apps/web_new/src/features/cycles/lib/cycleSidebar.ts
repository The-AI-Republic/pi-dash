// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle sidebar model (parity/specs/cycles.md §14, CYC-035, CYC-038,
// CYC-039). Pure: the collapse flag, the per-cycle breakdown tab, the
// progress counts, the chart gate, the burndown points, and the
// breakdown-entry filter toggle. Components read these; storage and
// queries stay with the callers so every rule unit-tests without a DOM.

import type { CycleCompletionChart, CycleDistribution, CycleProgress } from "@pidash/api-client";

import type { AreaPoint } from "../../../shared/charts/series.js";
import { isCompletedCycle } from "../../../shared/pickers/cycles.js";
import {
  conditionKey,
  findField,
  formatDay,
  richFilterFields,
  selectedIds,
  togglePick,
  type RichCondition,
} from "../../issues/index.js";
import { instantToProjectDate, projectToday } from "./cycleDates.js";

/** Collapse flag (spec §14: the string "true" means collapsed). */
export const CYCLE_SIDEBAR_COLLAPSED_KEY = "cycle_sidebar_collapsed";

/** Narrow storage surface so tests inject a map instead of localStorage. */
export interface SidebarFlagStorage {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
}

/** True only when the stored flag is exactly "true" (spec §14). */
export function readSidebarCollapsed(storage: SidebarFlagStorage): boolean {
  try {
    return storage.getItem(CYCLE_SIDEBAR_COLLAPSED_KEY) === "true";
  } catch {
    return false;
  }
}

/** Persist the collapse choice; expanding drops the key (absent = expanded). */
export function writeSidebarCollapsed(storage: SidebarFlagStorage, collapsed: boolean): void {
  try {
    if (collapsed) storage.setItem(CYCLE_SIDEBAR_COLLAPSED_KEY, "true");
    else storage.removeItem(CYCLE_SIDEBAR_COLLAPSED_KEY);
  } catch {
    // Private modes keep the in-memory state usable without persisting.
  }
}

/** Breakdown tabs in spec order (spec §14). */
export const CYCLE_BREAKDOWN_TABS = ["States", "Assignees", "Labels"] as const;
export type CycleBreakdownTab = (typeof CYCLE_BREAKDOWN_TABS)[number];

/** Stored tab falls back here (spec §14). */
export const DEFAULT_BREAKDOWN_TAB: CycleBreakdownTab = "Assignees";

/** Per-cycle tab key (spec §14). */
export function breakdownTabKey(cycleId: string): string {
  return `cycle-analytics-tab-${cycleId}`;
}

function isBreakdownTab(value: unknown): value is CycleBreakdownTab {
  return value === "States" || value === "Assignees" || value === "Labels";
}

/** The stored tab for one cycle, or the Assignees default. */
export function readBreakdownTab(storage: SidebarFlagStorage, cycleId: string): CycleBreakdownTab {
  try {
    const stored = storage.getItem(breakdownTabKey(cycleId));
    return isBreakdownTab(stored) ? stored : DEFAULT_BREAKDOWN_TAB;
  } catch {
    return DEFAULT_BREAKDOWN_TAB;
  }
}

/** Remember the tab for one cycle. */
export function writeBreakdownTab(storage: SidebarFlagStorage, cycleId: string, tab: CycleBreakdownTab): void {
  try {
    storage.setItem(breakdownTabKey(cycleId), tab);
  } catch {
    // Private modes keep the in-memory tab usable without persisting.
  }
}

/** Chart/breakdown measure: the analytics `type` behind each label. */
export type CycleProgressMeasure = "issues" | "points";

/** Measure options in oracle order (spec §14, §26). */
export const CYCLE_PROGRESS_MEASURES: readonly { value: CycleProgressMeasure; label: string }[] = [
  { value: "issues", label: "Work items" },
  { value: "points", label: "Estimates" },
];

/** Visible label behind a measure value. */
export function measureLabel(measure: CycleProgressMeasure): string {
  return measure === "points" ? "Estimates" : "Work items";
}

/** One state-group entry: its filter key, title, count, grand total, share. */
export interface CycleStateGroupRow {
  key: string;
  title: string;
  count: number;
  total: number;
  percent: number;
}

/** State groups in breakdown order; cancelled renders only with items. */
const STATE_GROUP_ORDER: readonly { key: string; title: string }[] = [
  { key: "backlog", title: "Backlog" },
  { key: "unstarted", title: "Unstarted" },
  { key: "started", title: "Started" },
  { key: "completed", title: "Completed" },
];

function groupCount(progress: CycleProgress, key: string, measure: CycleProgressMeasure): number {
  if (measure === "points") {
    switch (key) {
      case "backlog":
        return progress.backlog_estimate_points;
      case "unstarted":
        return progress.unstarted_estimate_points;
      case "started":
        return progress.started_estimate_points;
      case "completed":
        return progress.completed_estimate_points;
      default:
        return progress.cancelled_estimate_points;
    }
  }
  switch (key) {
    case "backlog":
      return progress.backlog_issues;
    case "unstarted":
      return progress.unstarted_issues;
    case "started":
      return progress.started_issues;
    case "completed":
      return progress.completed_issues;
    default:
      return progress.cancelled_issues;
  }
}

/** Grand total behind the shares (cancelled included, spec §14). */
export function progressGrandTotal(progress: CycleProgress, measure: CycleProgressMeasure): number {
  return measure === "points" ? (progress.total_estimate_points ?? 0) : progress.total_issues;
}

/** Rounded share of the grand total; a zero total shares nothing. */
export function progressShare(count: number, total: number): number {
  if (total <= 0 || count <= 0) return 0;
  return Math.round((count / total) * 100);
}

/**
 * State-group rows for the active measure: the four open groups always,
 * cancelled only when it holds items (spec §14).
 */
export function selectStateGroupRows(progress: CycleProgress, measure: CycleProgressMeasure): CycleStateGroupRow[] {
  const total = progressGrandTotal(progress, measure);
  const rows = STATE_GROUP_ORDER.map(({ key, title }) => {
    const count = groupCount(progress, key, measure);
    return { key, title, count, total, percent: progressShare(count, total) };
  });
  const cancelled = groupCount(progress, "cancelled", measure);
  if (cancelled > 0) {
    rows.push({
      key: "cancelled",
      title: "Cancelled",
      count: cancelled,
      total,
      percent: progressShare(cancelled, total),
    });
  }
  return rows;
}

/** One assignee/label entry: its filter id, title, count, grand total, share. */
export interface CycleDistributionRow {
  id: string | null;
  title: string;
  count: number;
  total: number;
  percent: number;
}

function distributionCount(
  entry: { total_issues?: number | null | undefined; total_estimates?: number | null | undefined },
  measure: CycleProgressMeasure
): number {
  const value = measure === "points" ? entry.total_estimates : entry.total_issues;
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/** Assignee entries for the active measure, in server order. */
export function selectAssigneeRows(
  distribution: CycleDistribution,
  measure: CycleProgressMeasure,
  grandTotal: number
): CycleDistributionRow[] {
  return distribution.assignees.map((entry) => {
    const count = distributionCount(entry, measure);
    return {
      id: entry.assignee_id ?? null,
      title: entry.display_name ?? "Unassigned",
      count,
      total: grandTotal,
      percent: progressShare(count, grandTotal),
    };
  });
}

/** Label entries for the active measure, in server order. */
export function selectLabelRows(
  distribution: CycleDistribution,
  measure: CycleProgressMeasure,
  grandTotal: number
): CycleDistributionRow[] {
  return distribution.labels.map((entry) => {
    const count = distributionCount(entry, measure);
    return {
      id: entry.label_id ?? null,
      title: entry.label_name ?? "Unlabeled",
      count,
      total: grandTotal,
      percent: progressShare(count, grandTotal),
    };
  });
}

/**
 * Whether the burndown chart renders: the cycle carries both dates and
 * the start is not in the future (spec §14: undated and future-start
 * cycles hide it; the comparison reads the project zone).
 */
export function cycleProgressChartVisible(
  startDate: string | null,
  endDate: string | null,
  timeZone: string | null | undefined,
  now: Date = new Date()
): boolean {
  if (startDate === null || endDate === null) return false;
  const start = instantToProjectDate(startDate, timeZone);
  if (start === null) return false;
  return start <= projectToday(timeZone, now);
}

/** Undated cycles show the empty notice instead of the chart (spec §14). */
export const CYCLE_PROGRESS_EMPTY_TEXT = "No Data yet";

/** The chart empty notice, or null when the chart area stays blank. */
export function cycleProgressEmptyText(startDate: string | null, endDate: string | null): string | null {
  return startDate === null && endDate === null ? CYCLE_PROGRESS_EMPTY_TEXT : null;
}

/**
 * Burndown points, oldest first: one per dated reading, future nulls
 * dropped so the plot stops at today (spec §14, §23).
 */
export function completionChartPoints(chart: CycleCompletionChart): AreaPoint[] {
  return Object.entries(chart)
    .filter((entry): entry is [string, number] => typeof entry[1] === "number")
    .sort(([left], [right]) => (left < right ? -1 : 1))
    .map(([date, remaining]) => ({ label: formatDay(date), values: { remaining } }));
}

/**
 * The Work items pair (`completed/total`, cancelled included) or the
 * zero-total string (spec §14); the pair is the proven contract.
 */
export function workItemsPairText(progress: CycleProgress): string {
  if (progress.total_issues <= 0) return "0 Work item";
  return `${progress.completed_issues}/${progress.total_issues}`;
}

/** The Points pair (`completed/total` estimate points, spec §14). */
export function pointsPairText(progress: CycleProgress): string {
  return `${progress.completed_estimate_points}/${progress.total_estimate_points ?? 0}`;
}

/** Frozen snapshot behind the finished-cycle Points gate. */
export interface CyclePointsSnapshot {
  progress_snapshot?: Record<string, unknown> | null | undefined;
}

/**
 * Whether the Points row renders: the project's estimate system is
 * points, the read carries estimate totals, the cycle is not completed —
 * and a finished cycle's snapshot still gates on its own
 * estimate_distribution (spec §14).
 */
export function showPointsRow(input: {
  estimateType: string | null | undefined;
  status: string | undefined;
  totalEstimatePoints: number | null;
  snapshot: CyclePointsSnapshot;
}): boolean {
  if (input.estimateType !== "points") return false;
  if (input.totalEstimatePoints === null) return false;
  if (!isCompletedCycle(input.status)) return true;
  const distribution = input.snapshot.progress_snapshot?.["estimate_distribution"];
  return distribution !== null && distribution !== undefined;
}

/**
 * Whether the progress measure picker renders (spec §14, EST-019).
 * The picker shows when the project has estimates enabled or the cycle
 * already carries point data. A categories system is text-family: the
 * picker is absent there, never rendered disabled. Every other family,
 * including time, keeps the estimates measure when enabled.
 */
export function showProgressMeasure(input: {
  estimatesEnabled: boolean;
  estimateType: string | null | undefined;
  progress: CycleProgress;
}): boolean {
  const { estimatesEnabled, estimateType, progress } = input;
  if (estimateType === "categories") return false;
  if (estimatesEnabled) return true;
  return (progress.total_estimate_points ?? 0) > 0;
}

/** Rich-filter stem behind each breakdown tab (spec §14). */
export const BREAKDOWN_FILTER_STEMS: Record<CycleBreakdownTab, string> = {
  States: "state_group",
  Assignees: "assignee_id",
  Labels: "label_id",
};

/**
 * Toggle one breakdown entry in the cycle's rich conditions: a first
 * click adds the value to the tab's stem, a repeat lifts it (dropping
 * the condition when it empties), and other stems ride untouched, so
 * entries narrow, combine and lift (spec §14, CYC-039).
 */
export function toggleBreakdownCondition(
  conditions: readonly RichCondition[],
  stem: string,
  id: string
): RichCondition[] {
  const field = findField(richFilterFields(), stem);
  if (field === null) return [...conditions];
  const index = conditions.findIndex(
    (condition) => condition.property === stem && condition.operator === "in" && !condition.negated
  );
  if (index === -1) {
    return [...conditions, { key: conditionKey(), property: stem, operator: "in", negated: false, value: [id] }];
  }
  const next = togglePick(field, conditions[index]!.value, id);
  if (next === null) return conditions.filter((_, position) => position !== index);
  return conditions.map((condition, position) => (position === index ? { ...condition, value: next } : condition));
}

/** Whether one breakdown entry is currently applied. */
export function breakdownEntryActive(conditions: readonly RichCondition[], stem: string, id: string): boolean {
  const condition = conditions.find((entry) => entry.property === stem && entry.operator === "in" && !entry.negated);
  if (condition === undefined) return false;
  return selectedIds(condition.value).includes(id);
}
