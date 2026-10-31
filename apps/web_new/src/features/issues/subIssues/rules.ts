// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issues widget rules (parity/specs/issues.md §15, ISS-173–178 and
// §22 ISS-174/238). Pure shaping only: the paged envelope and when a
// parent has another page, the progress rollup over the full child set,
// the filter vocabulary and its matching, ordering and grouping, the
// display properties a row draws, and the overdue and merged-range
// reads. Components and queries consume these; nothing here touches
// React or the network.

import type { SubIssueItem, SubIssuesResponse } from "@pidash/api-client";

import { PRIORITY_LABELS, PRIORITY_LEVELS, type PriorityLevel } from "../../../shared/pickers/priorities.js";
import type { StateGroup, StateOption } from "../../../shared/pickers/states.js";

/** First page size; the endpoint's default and the target's cap (ISS-238). */
export const SUB_ISSUE_PAGE_SIZE = 50;
/** Largest page the endpoint accepts. */
export const SUB_ISSUE_MAX_PAGE_SIZE = 100;

/** One widget title per kind of parent. */
export const SUB_ISSUE_TITLE = "Sub-work items";
/** Epic parents read as work items (ISS-174). */
export const EPIC_SUB_ISSUE_TITLE = "Work items";

export function subIssueTitle(isEpic: boolean): string {
  return isEpic ? EPIC_SUB_ISSUE_TITLE : SUB_ISSUE_TITLE;
}

/** Filter dimensions the widget offers, in render order. */
export type SubIssueFilterKey =
  | "priority"
  | "state_group"
  | "state"
  | "project"
  | "assignees"
  | "start_date"
  | "target_date";

export const SUB_ISSUE_FILTER_KEYS: readonly SubIssueFilterKey[] = [
  "priority",
  "state_group",
  "state",
  "project",
  "assignees",
  "start_date",
  "target_date",
];

/** Applied values per dimension; an empty list means the dimension is off. */
export type SubIssueFilters = Record<SubIssueFilterKey, string[]>;

export const SUB_ISSUE_FILTER_LABELS: Record<SubIssueFilterKey, string> = {
  priority: "Priority",
  state_group: "State group",
  state: "State",
  project: "Project",
  assignees: "Assignees",
  start_date: "Start date",
  target_date: "Due date",
};

export function emptySubIssueFilters(): SubIssueFilters {
  return {
    priority: [],
    state_group: [],
    state: [],
    project: [],
    assignees: [],
    start_date: [],
    target_date: [],
  };
}

/** Toggle one value in a dimension, returning a new filter object. */
export function toggleSubIssueFilter(filters: SubIssueFilters, key: SubIssueFilterKey, value: string): SubIssueFilters {
  const current = filters[key];
  const next = current.includes(value) ? current.filter((entry) => entry !== value) : [...current, value];
  return { ...filters, [key]: next };
}

/** Replace a dimension's values wholesale (the date footer edits this way). */
export function setSubIssueFilter(
  filters: SubIssueFilters,
  key: SubIssueFilterKey,
  values: readonly string[]
): SubIssueFilters {
  return { ...filters, [key]: [...values] };
}

/** How many dimensions carry an applied value. */
export function appliedFilterCount(filters: SubIssueFilters): number {
  return SUB_ISSUE_FILTER_KEYS.filter((key) => filters[key].length > 0).length;
}

export function hasActiveFilters(filters: SubIssueFilters): boolean {
  return appliedFilterCount(filters) > 0;
}

/** Grouping dimensions, in the order the display menu lists them. */
export type SubIssueGroupBy = "state" | "priority" | "assignees" | null;

/** Ordering dimensions the widget offers (the old sub-issue list). */
export type SubIssueOrderBy = "-created_at" | "-updated_at" | "start_date" | "-priority";

export const SUB_ISSUE_GROUP_OPTIONS: readonly { id: string; value: SubIssueGroupBy; label: string }[] = [
  { id: "state", value: "state", label: "States" },
  { id: "priority", value: "priority", label: "Priority" },
  { id: "assignees", value: "assignees", label: "Assignees" },
  { id: "none", value: null, label: "None" },
];

export const SUB_ISSUE_ORDER_OPTIONS: readonly { id: SubIssueOrderBy; label: string }[] = [
  { id: "-created_at", label: "Last created" },
  { id: "-updated_at", label: "Last updated" },
  { id: "start_date", label: "Start date" },
  { id: "-priority", label: "Priority" },
];

/** Columns a row can show, in render order (the old sub-issue defaults). */
export type SubIssuePropertyKey = "key" | "state" | "priority" | "assignee" | "start_date" | "due_date";

export const SUB_ISSUE_PROPERTY_KEYS: readonly SubIssuePropertyKey[] = [
  "key",
  "state",
  "priority",
  "assignee",
  "start_date",
  "due_date",
];

export const SUB_ISSUE_PROPERTY_LABELS: Record<SubIssuePropertyKey, string> = {
  key: "ID",
  state: "State",
  priority: "Priority",
  assignee: "Assignee",
  start_date: "Start date",
  due_date: "Due date",
};

/** Which property columns a row draws; absent reads as shown. */
export type SubIssueDisplayProperties = Record<SubIssuePropertyKey, boolean>;

export function defaultDisplayProperties(): SubIssueDisplayProperties {
  return {
    key: true,
    state: true,
    priority: true,
    assignee: true,
    start_date: true,
    due_date: true,
  };
}

/** The per-parent display state: grouping, ordering and columns. */
export interface SubIssueDisplay {
  groupBy: SubIssueGroupBy;
  orderBy: SubIssueOrderBy;
  properties: SubIssueDisplayProperties;
}

export function defaultSubIssueDisplay(): SubIssueDisplay {
  return { groupBy: null, orderBy: "-created_at", properties: defaultDisplayProperties() };
}

export function isPropertyVisible(display: SubIssueDisplayProperties, key: SubIssuePropertyKey): boolean {
  return display[key] !== false;
}

export function toggleDisplayProperty(
  display: SubIssueDisplayProperties,
  key: SubIssuePropertyKey
): SubIssueDisplayProperties {
  return { ...display, [key]: display[key] === false };
}

/** How many differences from the defaults the display menu reports. */
export function displayChangeCount(display: SubIssueDisplay): number {
  let count = display.groupBy === null ? 0 : 1;
  if (display.orderBy !== "-created_at") count += 1;
  for (const key of SUB_ISSUE_PROPERTY_KEYS) {
    if (display.properties[key] === false) count += 1;
  }
  return count;
}

/** Rows of one page (the envelope's `sub_issues`). */
export function pageRows(page: SubIssuesResponse): SubIssueItem[] {
  return page.sub_issues;
}

/** Total children across the full set: the paginated total first (ISS-238). */
export function subIssueTotal(page: SubIssuesResponse): number {
  if (page.total_count !== undefined) return page.total_count;
  const distribution = Object.values(page.state_distribution);
  if (distribution.length > 0) return distribution.reduce((sum, ids) => sum + ids.length, 0);
  return page.sub_issues.length;
}

/**
 * Progress rollup (ISS-174): completed children over the full-set total,
 * so every page reports the same numbers. No children reads as zero.
 */
export function subIssueRollup(page: SubIssuesResponse): { done: number; total: number; percent: number } {
  const done = page.state_distribution["completed"]?.length ?? 0;
  const total = subIssueTotal(page);
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
  return { done, total, percent };
}

/** The cursor for the next page, or null when the page closes the set. */
export function nextSubIssueCursor(page: SubIssuesResponse): string | null {
  const cursor = page.next_cursor;
  if (cursor === undefined || cursor === "") return null;
  if (page.next_page_results === false) return null;
  return cursor;
}

/**
 * Whether the parent has no further page. A short page is exhausted even
 * before the backend grows cursors; an explicit false or an absent cursor
 * closes it too (ISS-238).
 */
export function subIssuesExhausted(page: SubIssuesResponse, pageSize = SUB_ISSUE_PAGE_SIZE): boolean {
  if (nextSubIssueCursor(page) === null) return true;
  return page.sub_issues.length < pageSize;
}

/**
 * One parsed date entry: an inclusive bound on a calendar day. `on`
 * pins the exact day, `after`/`before` keep one open side.
 */
export interface SubIssueDateBound {
  type: "on" | "after" | "before";
  day: string;
}

const RELATIVE_ENTRY = /^(\d+)_(days|weeks|months);(after|before);fromnow$/;

function isoDay(date: Date): string {
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/**
 * One date-filter entry resolved against `today`: a relative window
 * (`<n>_<unit>s;after|before;fromnow`), an absolute day carrying its
 * direction (`<day>;after|before`), or a bare day, which means exactly
 * that day (`<day>`). Anything else is ignored rather than failing the
 * whole dimension.
 */
export function parseDateBound(entry: string, today: Date): SubIssueDateBound | null {
  const trimmed = entry.trim();
  const relative = RELATIVE_ENTRY.exec(trimmed);
  if (relative !== null) {
    const amount = Number(relative[1]);
    const unit = relative[2];
    const type = relative[3] as "after" | "before";
    const base = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate()));
    // "Before N" reaches into the past and "after N" into the future.
    // A month counts as thirty days, the filter's own convention.
    const days = unit === "days" ? amount : unit === "weeks" ? amount * 7 : amount * 30;
    base.setUTCDate(base.getUTCDate() + (type === "after" ? days : -days));
    return { type, day: isoDay(base) };
  }
  const [day, direction] = trimmed.split(";");
  if (day === undefined || !/^\d{4}-\d{2}-\d{2}$/.test(day)) return null;
  if (direction === undefined) return { type: "on", day };
  if (direction !== "after" && direction !== "before") return null;
  return { type: direction, day };
}

/** Every applied date entry must hold (AND inside the dimension). */
export function matchesDateBounds(iso: string | null | undefined, entries: readonly string[], today: Date): boolean {
  if (entries.length === 0) return true;
  const bounds = entries
    .map((entry) => parseDateBound(entry, today))
    .filter((bound): bound is SubIssueDateBound => bound !== null);
  if (bounds.length === 0) return true;
  if (iso === null || iso === undefined) return false;
  const value = iso.slice(0, 10);
  return bounds.every((bound) => {
    if (bound.type === "on") return value === bound.day;
    return bound.type === "after" ? value >= bound.day : value <= bound.day;
  });
}

function matchesAny(value: string | null, applied: readonly string[]): boolean {
  return applied.length === 0 || (value !== null && applied.includes(value));
}

function matchesAssignees(ids: readonly string[], applied: readonly string[]): boolean {
  return applied.length === 0 || applied.some((id) => ids.includes(id));
}

/**
 * Filter one page's rows (ISS-178): dimensions AND together, values OR
 * inside a dimension. Date dimensions compare the day only, inclusively,
 * and a child with no value on an active dimension drops out.
 */
export function filterSubIssues(rows: readonly SubIssueItem[], filters: SubIssueFilters, today: Date): SubIssueItem[] {
  return rows.filter((row) => {
    if (!matchesAny(row.priority, filters.priority)) return false;
    if (!matchesAny(row.state_group ?? null, filters.state_group)) return false;
    if (!matchesAny(row.state_id, filters.state)) return false;
    if (!matchesAny(row.project_id, filters.project)) return false;
    if (!matchesAssignees(row.assignee_ids, filters.assignees)) return false;
    if (!matchesDateBounds(row.start_date, filters.start_date, today)) return false;
    if (!matchesDateBounds(row.target_date, filters.target_date, today)) return false;
    return true;
  });
}

// The priority ladder reads urgent first; the rank follows the shared
// picker list so a new level cannot drift here.
const PRIORITY_ORDER: readonly PriorityLevel[] = PRIORITY_LEVELS;
const PRIORITY_RANK = new Map(PRIORITY_ORDER.map((level, index) => [level, index]));

/** Newest timestamp first; rows without a value sort last. */
function byTimestampDesc(key: "created_at" | "updated_at") {
  return (left: SubIssueItem, right: SubIssueItem): number => {
    const leftValue = left[key] ?? "";
    const rightValue = right[key] ?? "";
    if (leftValue === rightValue) return 0;
    return leftValue > rightValue ? -1 : 1;
  };
}

/** Order one group's rows; the sort is stable in the specified key. */
export function orderSubIssues(rows: readonly SubIssueItem[], orderBy: SubIssueOrderBy): SubIssueItem[] {
  const ordered = [...rows];
  switch (orderBy) {
    case "-updated_at":
      return ordered.sort(byTimestampDesc("updated_at"));
    case "start_date":
      // Dated rows first, earliest first; undated stay at the bottom.
      return ordered.sort((left, right) => {
        const leftDate = left.start_date ?? null;
        const rightDate = right.start_date ?? null;
        if (leftDate === rightDate) return 0;
        if (leftDate === null) return 1;
        if (rightDate === null) return -1;
        return leftDate < rightDate ? -1 : 1;
      });
    case "-priority":
      return ordered.sort(
        (left, right) =>
          (PRIORITY_RANK.get(left.priority) ?? PRIORITY_LEVELS.length) -
          (PRIORITY_RANK.get(right.priority) ?? PRIORITY_LEVELS.length)
      );
    case "-created_at":
    default:
      return ordered.sort(byTimestampDesc("created_at"));
  }
}

/** A group's identity plus the rows it holds, in order. */
export interface SubIssueGroup {
  id: string;
  label: string;
  rows: SubIssueItem[];
}

/** What the caller knows about the grouping universe (states, members). */
export interface GroupUniverse {
  /** Project workflow states, in server order. */
  states?: readonly StateOption[] | undefined;
  /** Member ids in picker order. */
  memberIds?: readonly string[] | undefined;
  /** Names for member ids; falls back to the id. */
  memberNames?: Readonly<Record<string, string>> | undefined;
}

const NONE_GROUP = "None";

function stateGroupOf(row: SubIssueItem, universe: GroupUniverse): { id: string; label: string } {
  const state = universe.states?.find((entry) => entry.id === row.state_id);
  if (state === undefined) return { id: NONE_GROUP, label: NONE_GROUP };
  return { id: state.id, label: state.name };
}

function priorityGroupOf(row: SubIssueItem): { id: string; label: string } {
  return { id: row.priority, label: row.priority === "none" ? NONE_GROUP : PRIORITY_LABELS[row.priority] };
}

function assigneeGroupOf(row: SubIssueItem, universe: GroupUniverse): { id: string; label: string } {
  // A child can carry several assignees; it lands in the group of its
  // first assignee in picker order, so a row never repeats.
  const ordered = universe.memberIds ?? row.assignee_ids;
  const first = ordered.find((id) => row.assignee_ids.includes(id));
  if (first === undefined) return { id: NONE_GROUP, label: NONE_GROUP };
  return { id: first, label: universe.memberNames?.[first] ?? first };
}

function groupKeyOf(row: SubIssueItem, groupBy: Exclude<SubIssueGroupBy, null>, universe: GroupUniverse) {
  if (groupBy === "state") return stateGroupOf(row, universe);
  if (groupBy === "priority") return priorityGroupOf(row);
  return assigneeGroupOf(row, universe);
}

/** Group header order: known universe order, unknown groups after, None last. */
function groupOrder(
  groupBy: Exclude<SubIssueGroupBy, null>,
  universe: GroupUniverse
): (group: { id: string; label: string }) => number {
  const orderedIds =
    groupBy === "state"
      ? (universe.states ?? []).map((state) => state.id)
      : groupBy === "priority"
        ? [...PRIORITY_LEVELS]
        : (universe.memberIds ?? []);
  const rank = new Map(orderedIds.map((id, index) => [id, index]));
  return (group) => {
    if (group.id === NONE_GROUP) return Number.MAX_SAFE_INTEGER;
    return rank.get(group.id) ?? orderedIds.length;
  };
}

/**
 * Group the page's rows (ISS-178, ISS-238: grouping applies to the
 * loaded page only). Groups that hold no row stay out; the ungrouped
 * display returns one group with an empty id.
 */
export function groupSubIssues(
  rows: readonly SubIssueItem[],
  groupBy: SubIssueGroupBy,
  orderBy: SubIssueOrderBy,
  universe: GroupUniverse = {}
): SubIssueGroup[] {
  if (groupBy === null) {
    return [{ id: "", label: "", rows: orderSubIssues(rows, orderBy) }];
  }
  const buckets = new Map<string, { label: string; rows: SubIssueItem[] }>();
  for (const row of rows) {
    const key = groupKeyOf(row, groupBy, universe);
    const bucket = buckets.get(key.id);
    if (bucket === undefined) buckets.set(key.id, { label: key.label, rows: [row] });
    else bucket.rows.push(row);
  }
  const groups = [...buckets.entries()].map(([id, bucket]) => ({
    id,
    label: bucket.label,
    rows: orderSubIssues(bucket.rows, orderBy),
  }));
  const rank = groupOrder(groupBy, universe);
  return groups.sort((left, right) => rank(left) - rank(right));
}

/** State groups that count as finished for the overdue read. */
const CLOSED_STATE_GROUPS: ReadonlySet<StateGroup> = new Set<StateGroup>(["completed", "cancelled"]);

/**
 * Overdue highlighting (ISS-177): a due day on or before today, on a
 * child that is not completed or cancelled.
 */
export function isSubIssueOverdue(row: SubIssueItem, today: Date): boolean {
  if (row.target_date === null || row.target_date === undefined) return false;
  const group = row.state_group as StateGroup | null;
  if (group !== null && CLOSED_STATE_GROUPS.has(group)) return false;
  return row.target_date.slice(0, 10) <= isoDay(today);
}

/**
 * The merged date range (ISS-177): drawn only when both ends are set and
 * both date columns are shown; otherwise each date keeps its own control.
 */
export function showsMergedDateRange(row: SubIssueItem, properties: SubIssueDisplayProperties): boolean {
  return (
    row.start_date !== null &&
    row.start_date !== undefined &&
    row.target_date !== null &&
    row.target_date !== undefined &&
    isPropertyVisible(properties, "start_date") &&
    isPropertyVisible(properties, "due_date")
  );
}
