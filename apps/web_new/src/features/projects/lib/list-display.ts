// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list display math (SHELL-028 through SHELL-031): sorting,
// narrowing, live search, match counts and person options over the
// fetched collection. Pure, so the view, the controls and the unit
// suites share every rule. Written from parity/specs/shell.md §3.

import type { ProjectDetails, WorkspaceMember } from "@pidash/api-client";

// --- sort (SHELL-028) -----------------------------------------------------

export type ListSortKey = "manual" | "name" | "created" | "members";
export type ListSortDirection = "ascending" | "descending";

export interface ListSort {
  key: ListSortKey;
  direction: ListSortDirection;
}

/** Reload and first visit land here: creation order, oldest first. */
export const DEFAULT_LIST_SORT: ListSort = { key: "created", direction: "ascending" };

export const LIST_SORT_OPTIONS: ReadonlyArray<{ key: ListSortKey; label: string }> = [
  { key: "manual", label: "Manual" },
  { key: "name", label: "Name" },
  { key: "created", label: "Created date" },
  { key: "members", label: "Number of members" },
];

export function sortKeyLabel(key: ListSortKey): string {
  return LIST_SORT_OPTIONS.find((option) => option.key === key)?.label ?? "Created date";
}

/** Manual order carries no direction; every other key flips both ways. */
export function sortDirectionLocked(sort: ListSort): boolean {
  return sort.key === "manual";
}

function compareText(left: string, right: string): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

/**
 * Order one view's rows. Stable: ties keep collection order. Manual
 * rows without an order slot sink last; descending mirrors ascending
 * (newest first for creation date).
 */
export function sortListRows(rows: ProjectDetails[], sort: ListSort): ProjectDetails[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  indexed.sort((left, right) => {
    if (sort.key === "manual") {
      const leftOrder = left.row.sort_order;
      const rightOrder = right.row.sort_order;
      if (leftOrder === null && rightOrder === null) return left.index - right.index;
      if (leftOrder === null) return 1;
      if (rightOrder === null) return -1;
      if (leftOrder !== rightOrder) return leftOrder - rightOrder;
      return left.index - right.index;
    }
    if (sort.key === "name") {
      const verdict = compareText(left.row.name.toLowerCase(), right.row.name.toLowerCase());
      return verdict === 0 ? left.index - right.index : verdict;
    }
    if (sort.key === "created") {
      const verdict = compareText(left.row.created_at, right.row.created_at);
      return verdict === 0 ? left.index - right.index : verdict;
    }
    const verdict = left.row.members.length - right.row.members.length;
    return verdict === 0 ? left.index - right.index : verdict;
  });
  const ordered = indexed.map((entry) => entry.row);
  if (sort.key !== "manual" && sort.direction === "descending") ordered.reverse();
  return ordered;
}

// --- filters (SHELL-029) ---------------------------------------------------

export interface ListFilters {
  myProjects: boolean;
  /** Network values: 0 private, 2 public. */
  access: number[];
  /** Lead member ids. */
  lead: string[];
  /** Member ids with any overlap counting. */
  members: string[];
  /** Creation-date values; one effective pick at a time. */
  createdAt: string[];
}

/** Person-valued panel dimensions. */
export type PersonFilterDimension = "lead" | "members";

/** Chip-valued strip dimensions. */
export type ValueFilterDimension = "access" | "lead" | "members" | "createdAt";

export const EMPTY_LIST_FILTERS: ListFilters = {
  myProjects: false,
  access: [],
  lead: [],
  members: [],
  createdAt: [],
};

export function freshListFilters(): ListFilters {
  return { myProjects: false, access: [], lead: [], members: [], createdAt: [] };
}

export const ACCESS_OPTIONS: ReadonlyArray<{ value: number; label: string }> = [
  { value: 0, label: "Private" },
  { value: 2, label: "Public" },
];

export const CREATED_PRESETS: ReadonlyArray<{ value: string; label: string }> = [
  { value: "today;custom;custom", label: "Today" },
  { value: "yesterday;custom;custom", label: "Yesterday" },
  { value: "last_7_days;custom;custom", label: "Last 7 days" },
  { value: "last_30_days;custom;custom", label: "Last 30 days" },
];

function toDayStart(value: string): Date | null {
  const text = value.slice(0, 10);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(text)) return null;
  const year = Number(text.slice(0, 4));
  const month = Number(text.slice(5, 7));
  const day = Number(text.slice(8, 10));
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return date;
}

const DAY_MS = 86_400_000;

/** Whole calendar days from `from` to `to`; negative counts the past. */
function calendarDayDiff(from: Date, to: Date): number {
  const start = new Date(from.getFullYear(), from.getMonth(), from.getDate());
  const end = new Date(to.getFullYear(), to.getMonth(), to.getDate());
  return Math.round((end.getTime() - start.getTime()) / DAY_MS);
}

/**
 * Whether one creation timestamp satisfies one date value. Presets
 * compare calendar days against today; absolute picks compare day
 * starts, so a range covers both endpoint days in full.
 */
export function matchesCreatedFilter(createdAt: string, filter: string, today: Date = new Date()): boolean {
  const created = toDayStart(createdAt);
  if (!created) return false;
  const [value, operator, from] = filter.split(";");
  if (operator === "custom" && from === "custom") {
    const diff = calendarDayDiff(today, created);
    if (value === "today") return diff === 0;
    if (value === "yesterday") return diff === -1;
    if (value === "last_7_days") return diff >= -7;
    if (value === "last_30_days") return diff >= -30;
    return false;
  }
  if ((from === undefined || from === "") && value !== undefined && operator !== undefined) {
    const boundary = toDayStart(value);
    if (!boundary) return false;
    if (operator === "after") return created.getTime() >= boundary.getTime();
    if (operator === "before") return created.getTime() <= boundary.getTime();
  }
  return false;
}

const MONTH_NAMES = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;

function formatDay(date: Date): string {
  const month = MONTH_NAMES[date.getMonth()] ?? "";
  return `${month} ${String(date.getDate()).padStart(2, "0")}, ${date.getFullYear()}`;
}

/** Chip text for one creation-date value: preset name or After/Before day. */
export function formatCreatedChip(filter: string): string {
  const preset = CREATED_PRESETS.find((option) => option.value === filter);
  if (preset) return preset.label;
  const [value, operator] = filter.split(";");
  if (value === undefined || (operator !== "after" && operator !== "before")) return "";
  const boundary = toDayStart(value);
  if (!boundary) return "";
  return `${operator === "after" ? "After" : "Before"} ${formatDay(boundary)}`;
}

/** Absolute picks (from the custom dialog) as opposed to presets. */
export function isCustomCreatedValue(filter: string): boolean {
  return !CREATED_PRESETS.some((option) => option.value === filter);
}

/**
 * Whether one row survives the panel filters. Dimensions combine with
 * AND; several picks inside access, lead or members combine with OR.
 */
export function matchesListFilters(project: ProjectDetails, filters: ListFilters): boolean {
  if (filters.access.length > 0 && !filters.access.includes(project.network)) return false;
  if (filters.lead.length > 0) {
    if (project.project_lead === null || !filters.lead.includes(project.project_lead)) return false;
  }
  if (filters.members.length > 0 && !filters.members.some((id) => project.members.includes(id))) return false;
  for (const filter of filters.createdAt) {
    if (!matchesCreatedFilter(project.created_at, filter)) return false;
  }
  if (filters.myProjects && !project.member_role) return false;
  return true;
}

// --- live search (SHELL-031) ------------------------------------------------

/** Name or identifier, case-insensitive; blank matches everything. */
export function matchesListSearch(project: ProjectDetails, query: string): boolean {
  const needle = query.toLowerCase();
  if (needle === "") return true;
  return project.name.toLowerCase().includes(needle) || project.identifier.toLowerCase().includes(needle);
}

/** Narrow one view's rows by panel filters plus the live search text. */
export function filterListRows(rows: ProjectDetails[], filters: ListFilters, searchQuery: string): ProjectDetails[] {
  return rows.filter((row) => matchesListSearch(row, searchQuery) && matchesListFilters(row, filters));
}

// --- strip counts (SHELL-030) ------------------------------------------------

/**
 * Picks driving the trigger badge: panel values only. The My projects
 * toggle narrows the list but never lights the badge.
 */
export function countActiveListFilters(filters: ListFilters): number {
  return filters.access.length + filters.lead.length + filters.members.length + filters.createdAt.length;
}

/** Whether anything besides the search text narrows the rows. */
export function hasNarrowingListFilters(filters: ListFilters): boolean {
  return filters.myProjects || countActiveListFilters(filters) > 0;
}

// --- person options (SHELL-029 lead/members sections) ------------------------

export interface MemberOption {
  id: string;
  displayName: string;
  avatarUrl: string;
  self: boolean;
  applied: boolean;
}

/**
 * Panel-ready people: value-searched by display name
 * (case-insensitively), applied first, then the caller, then by name.
 */
export function selectMemberOptions(
  memberships: WorkspaceMember[],
  args: { applied: string[]; selfId: string | null; query: string }
): MemberOption[] {
  const needle = args.query.toLowerCase();
  return memberships
    .filter((membership) => !membership.member.is_bot && membership.member.display_name.toLowerCase().includes(needle))
    .map((membership) => ({
      id: membership.member.id,
      displayName: membership.member.display_name,
      avatarUrl: membership.member.avatar_url ?? "",
      self: args.selfId !== null && membership.member.id === args.selfId,
      applied: args.applied.includes(membership.member.id),
    }))
    .sort((left, right) => {
      if (left.applied !== right.applied) return left.applied ? -1 : 1;
      if (left.self !== right.self) return left.self ? -1 : 1;
      return compareText(left.displayName.toLowerCase(), right.displayName.toLowerCase());
    });
}
