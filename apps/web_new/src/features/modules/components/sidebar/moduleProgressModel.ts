// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module progress section rules (parity/specs/modules.md §9.2, MOD-034).
// Pure: the date-state rule behind the default-open section and its
// notices, the header percentage, the measure labels, the breakdown
// row selectors (each tab reads its own dimension — the NEWFRONT-337
// fix), the burndown points, the per-module tab storage, and the
// breakdown-entry filter toggle. Archived records carry loose
// distributions, so every selector coerces unknown readings instead
// of trusting the live shapes.

import {
  conditionKey,
  findField,
  formatDay,
  richFilterFields,
  selectedIds,
  togglePick,
  type RichCondition,
} from "../../../issues/index.js";
import type { AreaPoint } from "../../../../shared/charts/series.js";
import { parseModuleDay, type ModuleZoneDay } from "../../lib/moduleDates.js";
import { localIsoDay } from "../../lib/moduleListControls.js";
import type { StorageLike } from "../../lib/listLayoutStore.js";

/** Breakdown tabs in strip order (spec §9.2). */
export const MODULE_BREAKDOWN_TABS = ["States", "Assignees", "Labels"] as const;
export type ModuleBreakdownTab = (typeof MODULE_BREAKDOWN_TABS)[number];

/** First-view tab: the default is Assignees, not the first strip tab. */
export const DEFAULT_MODULE_BREAKDOWN_TAB: ModuleBreakdownTab = "Assignees";

/** Both dates present but the module is not inside its window. */
export const MODULE_PROGRESS_INACTIVE_NOTICE = "This module isn't active yet.";

/** Either date missing or unreadable. */
export const MODULE_PROGRESS_INVALID_NOTICE = "Invalid date. Please enter valid date.";

/** The section's date reading: open chart, or which notice explains it. */
export type ModuleProgressDateState = "active" | "inactive" | "invalid";

function compareModuleZoneDay(left: ModuleZoneDay, right: ModuleZoneDay): number {
  if (left.year !== right.year) return left.year < right.year ? -1 : 1;
  if (left.month !== right.month) return left.month < right.month ? -1 : 1;
  if (left.day !== right.day) return left.day < right.day ? -1 : 1;
  return 0;
}

/**
 * Whether the section opens: the start is on or before today and the
 * target is not before the start (spec §9.2 — an overdue target still
 * opens). The notice branch follows only whether both dates parse, so a
 * start-after-target module reads inactive, never invalid.
 */
export function moduleProgressDateState(
  startDate: string | null | undefined,
  targetDate: string | null | undefined,
  todayIso: string = localIsoDay(new Date())
): ModuleProgressDateState {
  const start = parseModuleDay(startDate);
  const target = parseModuleDay(targetDate);
  const today = parseModuleDay(todayIso);
  if (start === null || target === null || today === null) return "invalid";
  if (compareModuleZoneDay(start, today) <= 0 && compareModuleZoneDay(target, start) >= 0) return "active";
  return "inactive";
}

/** The pinned notice behind a date state, or null when the chart shows. */
export function moduleProgressNotice(state: ModuleProgressDateState): string | null {
  if (state === "inactive") return MODULE_PROGRESS_INACTIVE_NOTICE;
  if (state === "invalid") return MODULE_PROGRESS_INVALID_NOTICE;
  return null;
}

/**
 * The header's completed share, rounded. A zero or missing side shares
 * nothing — the badge stays hidden instead of dividing by zero.
 */
export function moduleProgressHeaderPercent(
  completed: number | null | undefined,
  total: number | null | undefined
): number {
  if (typeof completed !== "number" || typeof total !== "number") return 0;
  if (!Number.isFinite(completed) || !Number.isFinite(total)) return 0;
  if (completed <= 0 || total <= 0) return 0;
  return Math.round((completed / total) * 100);
}

/** Chart/breakdown measure: work items, or estimate points. */
export type ModuleProgressMeasure = "issues" | "points";

/** Measure options in oracle order (spec §9.2). */
export const MODULE_PROGRESS_MEASURES: readonly { value: ModuleProgressMeasure; label: string }[] = [
  { value: "issues", label: "Work items" },
  { value: "points", label: "Points" },
];

/** Visible label behind a measure value. */
export function moduleProgressMeasureLabel(measure: ModuleProgressMeasure): string {
  return measure === "points" ? "Points" : "Work items";
}

/** Whole percent of a completed/total pair; a zero total shares nothing. */
export function moduleProgressShare(completed: number, total: number): number {
  if (!(total > 0) || !(completed > 0)) return 0;
  return Math.round((completed / total) * 100);
}

/** One state-group entry: its filter key, title, count, grand total, share. */
export interface ModuleStateGroupRow {
  key: string;
  title: string;
  count: number;
  total: number;
  percent: number;
}

/** State groups in breakdown order; every group renders, even at zero. */
const MODULE_STATE_GROUP_ORDER: readonly { key: string; title: string }[] = [
  { key: "backlog", title: "Backlog" },
  { key: "unstarted", title: "Unstarted" },
  { key: "started", title: "Started" },
  { key: "completed", title: "Completed" },
  { key: "cancelled", title: "Cancelled" },
];

/** Per-group counts behind the States tab, both measures. */
export interface ModuleStateGroupCounts {
  readonly backlog_issues: number;
  readonly unstarted_issues: number;
  readonly started_issues: number;
  readonly completed_issues: number;
  readonly cancelled_issues: number;
  readonly total_issues: number;
  readonly backlog_estimate_points?: number | null | undefined;
  readonly unstarted_estimate_points?: number | null | undefined;
  readonly started_estimate_points?: number | null | undefined;
  readonly completed_estimate_points?: number | null | undefined;
  readonly cancelled_estimate_points?: number | null | undefined;
  readonly total_estimate_points?: number | null | undefined;
}

function groupCount(counts: ModuleStateGroupCounts, key: string, measure: ModuleProgressMeasure): number {
  if (measure === "points") {
    switch (key) {
      case "backlog":
        return counts.backlog_estimate_points ?? 0;
      case "unstarted":
        return counts.unstarted_estimate_points ?? 0;
      case "started":
        return counts.started_estimate_points ?? 0;
      case "completed":
        return counts.completed_estimate_points ?? 0;
      default:
        return counts.cancelled_estimate_points ?? 0;
    }
  }
  switch (key) {
    case "backlog":
      return counts.backlog_issues;
    case "unstarted":
      return counts.unstarted_issues;
    case "started":
      return counts.started_issues;
    case "completed":
      return counts.completed_issues;
    default:
      return counts.cancelled_issues;
  }
}

/** Grand total behind the state shares for the active measure. */
export function moduleProgressGrandTotal(
  counts: Pick<ModuleStateGroupCounts, "total_issues" | "total_estimate_points">,
  measure: ModuleProgressMeasure
): number {
  return measure === "points" ? (counts.total_estimate_points ?? 0) : counts.total_issues;
}

/** State-group rows for the active measure, in breakdown order. */
export function selectModuleStateGroupRows(
  counts: ModuleStateGroupCounts,
  measure: ModuleProgressMeasure
): ModuleStateGroupRow[] {
  const total = moduleProgressGrandTotal(counts, measure);
  return MODULE_STATE_GROUP_ORDER.map(({ key, title }) => {
    const count = groupCount(counts, key, measure);
    return { key, title, count, total, percent: moduleProgressShare(count, total) };
  });
}

/** One assignee/label entry: its filter id, title, pair, share. */
export interface ModuleBreakdownRow {
  id: string | null;
  title: string;
  /** Label rows carry their color; state and assignee rows carry none. */
  color?: string | undefined;
  completed: number;
  total: number;
  percent: number;
}

/** Title of the unassigned row and the empty assignees tab. */
export const MODULE_NO_ASSIGNEE_TITLE = "No assignee";

/** Title of an unnamed label row and the empty labels tab. */
export const MODULE_NO_LABELS_TITLE = "No labels yet";

/**
 * Distribution entries as the selectors read them: live rows carry
 * typed fields, archived rows carry loose records, so every reading
 * is unknown until coerced.
 */
export interface ModuleAssigneeEntryLike {
  readonly assignee_id?: unknown;
  readonly display_name?: unknown;
  readonly total_issues?: unknown;
  readonly completed_issues?: unknown;
  readonly total_estimates?: unknown;
  readonly completed_estimates?: unknown;
}

/** Label entries, read the same lenient way. */
export interface ModuleLabelEntryLike {
  readonly label_id?: unknown;
  readonly label_name?: unknown;
  readonly color?: unknown;
  readonly total_issues?: unknown;
  readonly completed_issues?: unknown;
  readonly total_estimates?: unknown;
  readonly completed_estimates?: unknown;
}

/** A distribution object as the selectors read it (live or archived). */
export interface ModuleDistributionLike {
  readonly assignees?: unknown;
  readonly labels?: unknown;
  readonly completion_chart?: unknown;
}

function asCount(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function asId(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

function asTitle(value: unknown, fallback: string): string {
  return typeof value === "string" && value !== "" ? value : fallback;
}

function asColor(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

/** Entries of one distribution side; anything unshaped reads as empty. */
function distributionEntries<T extends ModuleAssigneeEntryLike | ModuleLabelEntryLike>(
  distribution: ModuleDistributionLike | undefined,
  side: "assignees" | "labels"
): readonly T[] {
  if (distribution === undefined) return [];
  const raw = distribution[side];
  if (!Array.isArray(raw)) return [];
  return raw.filter((entry): entry is T => typeof entry === "object" && entry !== null);
}

/** Assignee entries of one distribution side for the active measure. */
export function moduleAssigneeEntries(
  distribution: ModuleDistributionLike | undefined
): readonly ModuleAssigneeEntryLike[] {
  return distributionEntries<ModuleAssigneeEntryLike>(distribution, "assignees");
}

/** Label entries of one distribution side for the active measure. */
export function moduleLabelEntries(distribution: ModuleDistributionLike | undefined): readonly ModuleLabelEntryLike[] {
  return distributionEntries<ModuleLabelEntryLike>(distribution, "labels");
}

/** Assignee rows for the active measure, in server order. */
export function selectModuleAssigneeRows(
  entries: readonly ModuleAssigneeEntryLike[],
  measure: ModuleProgressMeasure
): ModuleBreakdownRow[] {
  return entries.map((entry) => {
    const completed = asCount(measure === "points" ? entry.completed_estimates : entry.completed_issues);
    const total = asCount(measure === "points" ? entry.total_estimates : entry.total_issues);
    return {
      id: asId(entry.assignee_id),
      title: asTitle(entry.display_name, MODULE_NO_ASSIGNEE_TITLE),
      completed,
      total,
      percent: moduleProgressShare(completed, total),
    };
  });
}

/** Label rows for the active measure, in server order. */
export function selectModuleLabelRows(
  entries: readonly ModuleLabelEntryLike[],
  measure: ModuleProgressMeasure
): ModuleBreakdownRow[] {
  return entries.map((entry) => {
    const completed = asCount(measure === "points" ? entry.completed_estimates : entry.completed_issues);
    const total = asCount(measure === "points" ? entry.total_estimates : entry.total_issues);
    return {
      id: asId(entry.label_id),
      title: asTitle(entry.label_name, MODULE_NO_LABELS_TITLE),
      color: asColor(entry.color),
      completed,
      total,
      percent: moduleProgressShare(completed, total),
    };
  });
}

/**
 * Burndown readings of one distribution side: dated numbers oldest
 * first, future nulls and anything unshaped dropped so the plot stops
 * at the last reading.
 */
export function moduleCompletionPoints(distribution: ModuleDistributionLike | undefined): AreaPoint[] {
  if (distribution === undefined) return [];
  const chart = distribution.completion_chart;
  if (typeof chart !== "object" || chart === null || Array.isArray(chart)) return [];
  return Object.entries(chart)
    .filter((entry): entry is [string, number] => typeof entry[1] === "number" && Number.isFinite(entry[1]))
    .sort(([left], [right]) => (left < right ? -1 : 1))
    .map(([date, remaining]) => ({ label: formatDay(date), values: { remaining } }));
}

/**
 * Whether the burndown chart renders: the side carries a completion
 * chart object. An empty chart still renders its (empty) plot; a
 * missing or unshaped one renders nothing.
 */
export function moduleCompletionChartPresent(distribution: ModuleDistributionLike | undefined): boolean {
  if (distribution === undefined) return false;
  const chart = distribution.completion_chart;
  return typeof chart === "object" && chart !== null && !Array.isArray(chart);
}

/** Browser record holding every module's breakdown tab. */
export const MODULE_PROGRESS_TABS_STORAGE_KEY = "modules_progress_tabs";

/** The store shape: module id → chosen tab. */
export type ModuleProgressTabRecord = Record<string, ModuleBreakdownTab>;

function defaultTabStorage(): StorageLike | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function isBreakdownTab(value: unknown): value is ModuleBreakdownTab {
  return value === "States" || value === "Assignees" || value === "Labels";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Parse a stored tab record, dropping unknown ids and unknown tabs. */
export function parseModuleProgressTabRecord(raw: string | null): ModuleProgressTabRecord {
  if (raw === null) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!isRecord(parsed)) return {};
  const record: ModuleProgressTabRecord = {};
  for (const [moduleId, value] of Object.entries(parsed)) {
    if (moduleId !== "" && isBreakdownTab(value)) record[moduleId] = value;
  }
  return record;
}

/** The full tab record currently stored (empty for an absent one). */
export function readModuleProgressTabRecord(
  storage: StorageLike | undefined = defaultTabStorage()
): ModuleProgressTabRecord {
  if (storage === undefined) return {};
  try {
    return parseModuleProgressTabRecord(storage.getItem(MODULE_PROGRESS_TABS_STORAGE_KEY));
  } catch {
    return {};
  }
}

/** One module's remembered tab; Assignees when never chosen. */
export function readModuleProgressTab(
  moduleId: string,
  storage: StorageLike | undefined = defaultTabStorage()
): ModuleBreakdownTab {
  return readModuleProgressTabRecord(storage)[moduleId] ?? DEFAULT_MODULE_BREAKDOWN_TAB;
}

/** Remember one module's tab, keeping every other module's entry. */
export function writeModuleProgressTab(
  moduleId: string,
  tab: ModuleBreakdownTab,
  storage: StorageLike | undefined = defaultTabStorage()
): void {
  if (storage === undefined) return;
  try {
    const record = readModuleProgressTabRecord(storage);
    record[moduleId] = tab;
    storage.setItem(MODULE_PROGRESS_TABS_STORAGE_KEY, JSON.stringify(record));
  } catch {
    // Private modes keep the in-memory tab usable without persisting.
  }
}

/** Rich-filter stem behind each breakdown tab (spec §9.2). */
export const MODULE_BREAKDOWN_FILTER_STEMS: Record<ModuleBreakdownTab, string> = {
  States: "state_group",
  Assignees: "assignee_id",
  Labels: "label_id",
};

/**
 * Toggle one breakdown entry in the module's rich conditions: a first
 * click adds the value to the tab's stem, a repeat lifts it (dropping
 * the condition when it empties), and other stems ride untouched.
 */
export function toggleModuleBreakdownCondition(
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
export function moduleBreakdownEntryActive(conditions: readonly RichCondition[], stem: string, id: string): boolean {
  const condition = conditions.find((entry) => entry.property === stem && entry.operator === "in" && !entry.negated);
  if (condition === undefined) return false;
  return selectedIds(condition.value).includes(id);
}
