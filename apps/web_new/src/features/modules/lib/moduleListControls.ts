// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules list header model (parity/specs/modules.md §2.4–§2.7, §3.3,
// MOD-004/005/006/007/010). Pure: the search reading, the six ordering
// keys and their direction rule, the filter dimensions and their
// predicates, the applied-chip groups, the member option shaping and the
// no-match decision. Nothing here touches React, the router or storage.

import type { ModuleRow, ModuleStatus, WorkspaceMember } from "@pidash/api-client";

// ---------------------------------------------------------------- search

/** Case-insensitive name substring match; blank query keeps every row. */
export function moduleMatchesSearch(row: Pick<ModuleRow, "name">, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return row.name.toLowerCase().includes(needle);
}

/** Narrow a module set by the name search (client-side, no request). */
export function searchModules<T extends Pick<ModuleRow, "name">>(rows: readonly T[], query: string): T[] {
  const needle = query.trim();
  if (needle === "") return [...rows];
  return rows.filter((row) => moduleMatchesSearch(row, needle));
}

// ---------------------------------------------------------------- sort

/** The six ordering keys, in menu order (§2.5). */
export const MODULE_SORT_KEYS = ["name", "progress", "work-items", "due-date", "created-date", "manual"] as const;
export type ModuleSortKey = (typeof MODULE_SORT_KEYS)[number];

export const MODULE_SORT_DIRECTIONS = ["ascending", "descending"] as const;
export type ModuleSortDirection = (typeof MODULE_SORT_DIRECTIONS)[number];

/** Pinned menu labels, in key order (§2.5). */
export const MODULE_SORT_LABELS: Record<ModuleSortKey, string> = {
  name: "Name",
  progress: "Progress",
  "work-items": "Number of work items",
  "due-date": "Due date",
  "created-date": "Created date",
  manual: "Manual",
};

/** One ordering choice; the default is Name ascending (§2.5). */
export interface ModuleSort {
  key: ModuleSortKey;
  direction: ModuleSortDirection;
}

export const DEFAULT_MODULE_SORT: ModuleSort = { key: "name", direction: "ascending" };

export const MODULE_SORT_OPTIONS: ReadonlyArray<{ key: ModuleSortKey; label: string }> = MODULE_SORT_KEYS.map(
  (key) => ({ key, label: MODULE_SORT_LABELS[key] })
);

export const MODULE_SORT_DIRECTION_LABELS: Record<ModuleSortDirection, string> = {
  ascending: "Ascending",
  descending: "Descending",
};

/** Manual placement has no direction: the stored order is used as-is. */
export function moduleSortHasDirection(key: ModuleSortKey): boolean {
  return key !== "manual";
}

export function isModuleSortKey(value: unknown): value is ModuleSortKey {
  return typeof value === "string" && (MODULE_SORT_KEYS as readonly string[]).includes(value);
}

export function isModuleSortDirection(value: unknown): value is ModuleSortDirection {
  return value === "ascending" || value === "descending";
}

/** Pick a key or direction; a Manual pick drops the direction to the default. */
export function selectModuleSort(sort: ModuleSort, choice: Partial<Pick<ModuleSort, "key" | "direction">>): ModuleSort {
  const key = choice.key ?? sort.key;
  const direction = moduleSortHasDirection(key) ? (choice.direction ?? sort.direction) : DEFAULT_MODULE_SORT.direction;
  return { key, direction };
}

/**
 * Natural, case-insensitive name comparison (§2.5): numeric runs compare
 * by value, so "Module 2" sorts before "Module 10". Numeric ties fall back
 * to the whole string; equal names return 0 for a stable sort.
 */
export function compareModuleNaturalNames(left: string, right: string): number {
  const chunk = /\d+|\D+/g;
  const leftParts = left.toLowerCase().match(chunk) ?? [];
  const rightParts = right.toLowerCase().match(chunk) ?? [];
  const length = Math.min(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const leftPart = leftParts[index]!;
    const rightPart = rightParts[index]!;
    const leftDigits = leftPart.charCodeAt(0) >= 48 && leftPart.charCodeAt(0) <= 57;
    const rightDigits = rightPart.charCodeAt(0) >= 48 && rightPart.charCodeAt(0) <= 57;
    if (leftDigits && rightDigits) {
      const leftValue = Number(leftPart);
      const rightValue = Number(rightPart);
      if (leftValue !== rightValue) return leftValue < rightValue ? -1 : 1;
      // "02" and "2" are equal by value; a shorter run reads first.
      if (leftPart.length !== rightPart.length) return leftPart.length < rightPart.length ? -1 : 1;
    } else if (leftPart !== rightPart) {
      return leftPart < rightPart ? -1 : 1;
    }
  }
  if (leftParts.length !== rightParts.length) return leftParts.length < rightParts.length ? -1 : 1;
  return 0;
}

/** Completion share for the Progress key; no work items reads as zero. */
export function moduleProgressShare(
  row: Pick<ModuleRow, "completed_issues" | "cancelled_issues" | "total_issues">
): number {
  if (row.total_issues <= 0) return 0;
  return (row.completed_issues + row.cancelled_issues) / row.total_issues;
}

function compareNumbers(left: number, right: number): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function compareInstants(left: string, right: string): number {
  const leftTime = Date.parse(left);
  const rightTime = Date.parse(right);
  if (Number.isNaN(leftTime) || Number.isNaN(rightTime)) {
    if (left === right) return 0;
    return left < right ? -1 : 1;
  }
  return compareNumbers(leftTime, rightTime);
}

/**
 * Due-date order with undated modules always last in both directions
 * (§2.5): only the dated segment reverses, and undated rows keep their
 * collection order among themselves.
 */
function compareDueDate(left: ModuleRow, right: ModuleRow, direction: ModuleSortDirection): number {
  const leftDated = left.target_date !== null;
  const rightDated = right.target_date !== null;
  if (leftDated !== rightDated) return leftDated ? -1 : 1;
  if (!leftDated || !rightDated) return 0;
  return direction === "ascending"
    ? compareInstants(left.target_date!, right.target_date!)
    : compareInstants(right.target_date!, left.target_date!);
}

/** The comparator for one ordering choice, exclusive of collection order. */
export function compareModulesBySort(left: ModuleRow, right: ModuleRow, sort: ModuleSort): number {
  const flip = sort.key === "manual" ? 1 : sort.direction === "descending" ? -1 : 1;
  switch (sort.key) {
    case "name":
      return flip * compareModuleNaturalNames(left.name, right.name);
    case "progress":
      return flip * compareNumbers(moduleProgressShare(left), moduleProgressShare(right));
    case "work-items":
      return flip * compareNumbers(left.total_issues, right.total_issues);
    case "due-date":
      return compareDueDate(left, right, sort.direction);
    case "created-date":
      return flip * compareInstants(left.created_at, right.created_at);
    case "manual":
      return compareNumbers(left.sort_order, right.sort_order);
  }
}

/**
 * Order a module set. Stable: collection (creation) order breaks ties, so
 * equal names and equal progress keep the server's own order (§2.5).
 */
export function orderModules<T extends ModuleRow>(rows: readonly T[], sort: ModuleSort): T[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  indexed.sort((left, right) => {
    const compared = compareModulesBySort(left.row, right.row, sort);
    return compared !== 0 ? compared : left.index - right.index;
  });
  return indexed.map((entry) => entry.row);
}

// ---------------------------------------------------------------- filters

/** Six lifecycle states in filter order, with their pinned labels (§2.6, §4.2). */
export const MODULE_STATUS_OPTIONS: ReadonlyArray<{ value: ModuleStatus; label: string; color: string }> = [
  { value: "backlog", label: "Backlog", color: "#a3a3a2" },
  { value: "planned", label: "Planned", color: "#3f76ff" },
  { value: "in-progress", label: "In Progress", color: "#f39e1f" },
  { value: "paused", label: "Paused", color: "#525252" },
  { value: "completed", label: "Completed", color: "#16a34a" },
  { value: "cancelled", label: "Cancelled", color: "#ef4444" },
];

/** The status color the dot and the chip read (§4.2). */
export function moduleStatusColor(status: ModuleStatus): string {
  return MODULE_STATUS_OPTIONS.find((option) => option.value === status)?.color ?? "#a3a3a2";
}

export function moduleStatusLabel(status: ModuleStatus): string {
  return MODULE_STATUS_OPTIONS.find((option) => option.value === status)?.label ?? status;
}

/** Relative date choices and their offsets, in menu order (§2.6). */
export const MODULE_DATE_PRESETS = [
  { value: "week-1", label: "1 week from now", offsetDays: 7 },
  { value: "week-2", label: "2 weeks from now", offsetDays: 14 },
  { value: "month-1", label: "1 month from now", offsetDays: 30 },
  { value: "month-2", label: "2 months from now", offsetDays: 60 },
] as const;
export type ModuleDatePreset = (typeof MODULE_DATE_PRESETS)[number]["value"];

const MODULE_DATE_PRESET_DAYS: Record<ModuleDatePreset, number> = Object.fromEntries(
  MODULE_DATE_PRESETS.map((option) => [option.value, option.offsetDays])
) as Record<ModuleDatePreset, number>;

const MODULE_DATE_PRESET_LABELS: Record<ModuleDatePreset, string> = Object.fromEntries(
  MODULE_DATE_PRESETS.map((option) => [option.value, option.label])
) as Record<ModuleDatePreset, string>;

/** The custom entry's label; it toggles a two-token bounded window. */
export const MODULE_DATE_CUSTOM_LABEL = "Custom";

/** Tokens inside a dimension. A preset is a lower bound; a range is two tokens. */
export type ModuleDateToken =
  | { kind: "preset"; value: ModuleDatePreset }
  | { kind: "range"; value: { start: string; end: string } };

/** One dimension's tokens; the empty list means no narrowing. */
export type ModuleDateTokens = ModuleDateToken[];

/** The filter dimensions, in menu and chip order (§2.6, §2.7). */
export const MODULE_FILTER_DIMENSIONS = ["favorites", "status", "lead", "members", "start-date", "due-date"] as const;
export type ModuleFilterDimension = (typeof MODULE_FILTER_DIMENSIONS)[number];

/** The dimension's stable chip key (§2.7). */
export const MODULE_FILTER_CHIP_KEYS: Record<ModuleFilterDimension, string> = {
  favorites: "Modules",
  status: "status",
  lead: "lead",
  members: "members",
  "start-date": "start date",
  "due-date": "target date",
};

export const MODULE_FILTER_LABELS: Record<ModuleFilterDimension, string> = {
  favorites: "Favorites",
  status: "Status",
  lead: "Lead",
  members: "Members",
  "start-date": "Start date",
  "due-date": "Due date",
};

export interface ModuleListFilters {
  favorites: boolean;
  status: ModuleStatus[];
  lead: string[];
  members: string[];
  "start-date": ModuleDateTokens;
  "due-date": ModuleDateTokens;
}

/** First visit: no narrowing at all (§2.6). */
export function emptyModuleFilters(): ModuleListFilters {
  return {
    favorites: false,
    status: [],
    lead: [],
    members: [],
    "start-date": [],
    "due-date": [],
  };
}

export function hasModuleFilters(filters: ModuleListFilters): boolean {
  return (
    filters.favorites ||
    filters.status.length > 0 ||
    filters.lead.length > 0 ||
    filters.members.length > 0 ||
    filters["start-date"].length > 0 ||
    filters["due-date"].length > 0
  );
}

/** A dimension is active when it holds any value. */
export function isModuleFilterDimensionActive(filters: ModuleListFilters, dimension: ModuleFilterDimension): boolean {
  const value = filters[dimension];
  if (dimension === "favorites") return value === true;
  return Array.isArray(value) && value.length > 0;
}

/** Toggle a scalar or multi-select value inside one dimension. */
export function toggleModuleFilterValue(filters: ModuleListFilters, dimension: "favorites"): ModuleListFilters;
export function toggleModuleFilterValue(
  filters: ModuleListFilters,
  dimension: "status",
  value: ModuleStatus
): ModuleListFilters;
export function toggleModuleFilterValue(
  filters: ModuleListFilters,
  dimension: "lead" | "members",
  value: string
): ModuleListFilters;
export function toggleModuleFilterValue(
  filters: ModuleListFilters,
  dimension: "start-date" | "due-date"
): ModuleListFilters;
export function toggleModuleFilterValue(
  filters: ModuleListFilters,
  dimension: ModuleFilterDimension,
  value?: string
): ModuleListFilters {
  if (dimension === "favorites") return { ...filters, favorites: !filters.favorites };
  if (dimension === "status") {
    const status = value as ModuleStatus;
    const has = filters.status.includes(status);
    return {
      ...filters,
      status: has ? filters.status.filter((entry) => entry !== status) : [...filters.status, status],
    };
  }
  if (dimension === "lead" || dimension === "members") {
    const id = value as string;
    const current = filters[dimension];
    return {
      ...filters,
      [dimension]: current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    };
  }
  return filters;
}

/** Toggle one date token in its dimension; identical tokens cancel out. */
export function toggleModuleDateToken(
  filters: ModuleListFilters,
  dimension: "start-date" | "due-date",
  token: ModuleDateToken
): ModuleListFilters {
  const current = filters[dimension];
  const matches = (entry: ModuleDateToken): boolean => {
    if (entry.kind !== token.kind) return false;
    return moduleDateTokenKey(entry) === moduleDateTokenKey(token);
  };
  return {
    ...filters,
    [dimension]: current.some(matches) ? current.filter((entry) => !matches(entry)) : [...current, token],
  };
}

/** Remove a range (the Custom entry's toggle) and keep relative tokens. */
export function clearModuleDateRange(
  filters: ModuleListFilters,
  dimension: "start-date" | "due-date"
): ModuleListFilters {
  return { ...filters, [dimension]: filters[dimension].filter((token) => token.kind !== "range") };
}

/** The first range token in a dimension, or null. */
export function moduleDateRange(
  filters: ModuleListFilters,
  dimension: "start-date" | "due-date"
): { start: string; end: string } | null {
  const token = filters[dimension].find((entry) => entry.kind === "range");
  return token !== undefined && token.kind === "range" ? token.value : null;
}

/** Remove one value from a dimension (chip value removal). */
export function removeModuleFilterValue(
  filters: ModuleListFilters,
  dimension: ModuleFilterDimension,
  value: string
): ModuleListFilters {
  if (dimension === "favorites") {
    return value === "Favorite" || value === "favorites" ? { ...filters, favorites: false } : filters;
  }
  if (dimension === "status") {
    return { ...filters, status: filters.status.filter((entry) => entry !== value) };
  }
  if (dimension === "lead" || dimension === "members") {
    return { ...filters, [dimension]: filters[dimension].filter((entry) => entry !== value) };
  }
  // Date values read as the rendered token key ("week-1" or "2026-10-20..2026-11-20").
  return {
    ...filters,
    [dimension]: filters[dimension].filter((token) => moduleDateTokenKey(token) !== value),
  };
}

/** Remove every value in one dimension. */
export function clearModuleFilterDimension(
  filters: ModuleListFilters,
  dimension: ModuleFilterDimension
): ModuleListFilters {
  return { ...filters, [dimension]: emptyModuleFilters()[dimension] };
}

export function clearModuleFilters(): ModuleListFilters {
  return emptyModuleFilters();
}

/** One day's ISO key in local time (the relative choices read local dates). */
export function localIsoDay(at: Date): string {
  const year = at.getFullYear();
  const month = `${at.getMonth() + 1}`.padStart(2, "0");
  const day = `${at.getDate()}`.padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Today plus `offsetDays`, in local calendar days. */
export function moduleDatePresetDay(preset: ModuleDatePreset, now: Date = new Date()): string {
  const offset = MODULE_DATE_PRESET_DAYS[preset];
  const day = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
  return localIsoDay(day);
}

/**
 * The custom dialog's default window: today through the same
 * day-of-month next month, letting the date constructor roll month-end
 * days into the next month (§2.6.1).
 */
export function defaultModuleCustomRange(now: Date = new Date()): { start: string; end: string } {
  const end = new Date(now.getFullYear(), now.getMonth() + 1, now.getDate());
  return { start: localIsoDay(now), end: localIsoDay(end) };
}

/** A stored target/start date (an ISO day) compared against a day. */
function dayAtOrAfter(value: string | null, day: string): boolean {
  return value !== null && value >= day;
}

function dayWithin(value: string | null, start: string, end: string): boolean {
  return value !== null && value >= start && value <= end;
}

/** Whether one date dimension's tokens all accept the module (§2.6). */
export function moduleMatchesDateDimension(
  row: Pick<ModuleRow, "start_date" | "target_date">,
  dimension: "start-date" | "due-date",
  tokens: ModuleDateTokens,
  now: Date = new Date()
): boolean {
  if (tokens.length === 0) return true;
  const value = dimension === "start-date" ? row.start_date : row.target_date;
  return tokens.every((token) => {
    if (token.kind === "preset") {
      return dayAtOrAfter(value, moduleDatePresetDay(token.value, now));
    }
    return dayWithin(value, token.value.start, token.value.end);
  });
}

/** Whether a module passes every filter group (AND across dimensions). */
export function moduleMatchesFilters(
  row: Pick<ModuleRow, "is_favorite" | "status" | "lead_id" | "member_ids" | "start_date" | "target_date">,
  filters: ModuleListFilters,
  now: Date = new Date()
): boolean {
  if (filters.favorites && !row.is_favorite) return false;
  if (filters.status.length > 0 && !filters.status.includes(row.status)) return false;
  if (filters.lead.length > 0 && (row.lead_id === null || !filters.lead.includes(row.lead_id))) return false;
  if (filters.members.length > 0 && !row.member_ids.some((id) => filters.members.includes(id))) return false;
  if (!moduleMatchesDateDimension(row, "start-date", filters["start-date"], now)) return false;
  if (!moduleMatchesDateDimension(row, "due-date", filters["due-date"], now)) return false;
  return true;
}

/**
 * The visible set: filter groups AND the name search (§2.6). Ordering is
 * a separate step so the list keeps its sort after narrowing.
 */
export function visibleModules<T extends ModuleRow>(
  rows: readonly T[],
  query: string,
  filters: ModuleListFilters,
  now: Date = new Date()
): T[] {
  return searchModules(rows, query).filter((row) => moduleMatchesFilters(row, filters, now));
}

// ---------------------------------------------------------------- chips

/** One removable value inside a dimension chip. */
export interface ModuleFilterChipValue {
  /** Stable removal key (member id, status value, preset value or range bounds). */
  value: string;
  /** Rendered text. */
  label: string;
  /** Optional avatar for people. */
  avatarUrl?: string | null;
  /** Optional status color for the status dot. */
  color?: string | undefined;
}

export interface ModuleFilterChipGroup {
  dimension: ModuleFilterDimension;
  /** Stable chip key the oracle reads ("status", "lead", "Modules", …). */
  key: string;
  /** Rendered dimension label. */
  label: string;
  values: ModuleFilterChipValue[];
}

/** Chip-group visibility props, so the archived screen hides favorites+status. */
export interface ModuleChipVisibility {
  showFavorites?: boolean | undefined;
  showStatus?: boolean | undefined;
}

/** Stable token key: a preset value, or the two bounds joined by "..". */
export function moduleDateTokenKey(token: ModuleDateToken): string {
  return token.kind === "preset" ? token.value : `${token.value.start}..${token.value.end}`;
}

function dateTokenLabel(token: ModuleDateToken, formatRange: (start: string, end: string) => string): string {
  return token.kind === "preset"
    ? MODULE_DATE_PRESET_LABELS[token.value]
    : formatRange(token.value.start, token.value.end);
}

/** One member option offered by the filter menu. */
export interface ModuleMemberOption {
  userId: string;
  /** "You" for the signed-in member, else the display name. */
  label: string;
  /** Text the menu search also matches (the real display name). */
  searchText: string;
  avatarUrl: string | null;
  selected: boolean;
}

export interface ModuleMemberOptionInput {
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  selectedIds: readonly string[];
  /** Guests are never offered. */
  excludeGuests?: boolean | undefined;
  /** The system account is never offered. */
  includeBots?: boolean | undefined;
}

/**
 * Member options in offer order: the signed-in member first (labelled
 * "You"), then selected members, then the directory order. Guests are
 * excluded and deactivated memberships stay offerable (the row shows them
 * suspended). This mirrors the picker's shaping rules; the modules menu
 * needs the same list without the picker's dropdown chrome.
 */
export function selectModuleMemberOptions({
  members,
  meId,
  selectedIds,
  excludeGuests = true,
  includeBots = false,
}: ModuleMemberOptionInput): ModuleMemberOption[] {
  if (members === undefined) return [];
  const options = members
    .filter((row) => !excludeGuests || row.role !== 5)
    .filter((row) => includeBots || !row.member.is_bot)
    .map((row) => {
      const displayName = row.member.display_name;
      return {
        userId: row.member.id,
        label: meId !== null && row.member.id === meId ? "You" : displayName,
        searchText: displayName,
        avatarUrl: row.member.avatar_url,
        selected: selectedIds.includes(row.member.id),
      };
    });
  options.sort((left, right) => {
    if (meId !== null) {
      if (left.userId === meId && right.userId !== meId) return -1;
      if (right.userId === meId && left.userId !== meId) return 1;
    }
    if (left.selected && !right.selected) return -1;
    if (right.selected && !left.selected) return 1;
    return 0;
  });
  return options;
}

/** One member's display name from the workspace directory, or the fallback. */
export function moduleMemberName(
  input: { members: readonly WorkspaceMember[] | undefined; meId?: string | null },
  userId: string
): string {
  const row = input.members?.find((entry) => entry.member.id === userId);
  if (row === undefined) return "Unknown member";
  if (input.meId !== null && input.meId !== undefined && input.meId === userId) return "You";
  return row.member.display_name;
}

/** Case-insensitive option narrowing for the menu's own search box. */
export function matchesModuleMenuQuery(label: string, searchText: string, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return `${label} ${searchText}`.toLowerCase().includes(needle);
}

export interface ModuleChipInput extends ModuleChipVisibility {
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  /** Renders a custom date range's two ends. */
  formatDateRange: (start: string, end: string) => string;
  /** Optional status color lookup. */
  statusColor?: ((status: ModuleStatus) => string | undefined) | undefined;
}

/**
 * The applied-chip groups, in chip order (§2.7). One group per active
 * dimension; empty when nothing is applied. The favorites group renders
 * under the key "Modules" with the value "Favorite".
 */
export function selectModuleChipGroups(filters: ModuleListFilters, input: ModuleChipInput): ModuleFilterChipGroup[] {
  const groups: ModuleFilterChipGroup[] = [];
  const showFavorites = input.showFavorites ?? true;
  const showStatus = input.showStatus ?? true;
  const people = selectModuleMemberOptions({ members: input.members, meId: input.meId, selectedIds: [] });

  const push = (dimension: ModuleFilterDimension, values: ModuleFilterChipValue[]): void => {
    if (values.length === 0) return;
    groups.push({
      dimension,
      key: MODULE_FILTER_CHIP_KEYS[dimension],
      label: MODULE_FILTER_LABELS[dimension],
      values,
    });
  };

  if (showFavorites && filters.favorites) {
    push("favorites", [{ value: "Favorite", label: "Favorite" }]);
  }
  if (showStatus) {
    push(
      "status",
      filters.status.map((status) => ({
        value: status,
        label: moduleStatusLabel(status),
        color: (input.statusColor ?? moduleStatusColor)(status),
      }))
    );
  }
  const peopleChip = (ids: readonly string[]): ModuleFilterChipValue[] =>
    ids.map((id) => {
      const option = people.find((entry) => entry.userId === id);
      return { value: id, label: option?.label ?? "Unknown member", avatarUrl: option?.avatarUrl ?? null };
    });
  push("lead", peopleChip(filters.lead));
  push("members", peopleChip(filters.members));
  push(
    "start-date",
    filters["start-date"].map((token) => ({
      value: moduleDateTokenKey(token),
      label: dateTokenLabel(token, input.formatDateRange),
    }))
  );
  push(
    "due-date",
    filters["due-date"].map((token) => ({
      value: moduleDateTokenKey(token),
      label: dateTokenLabel(token, input.formatDateRange),
    }))
  );
  return groups;
}

// ---------------------------------------------------------------- no match

/** The one body the list renders once the module set is settled. */
export type ModuleListContentState = "rows" | "no-match";

/**
 * Modules exist but the search/filters exclude every one (§3.3). An empty
 * project is the zero state's business (slice F), never this container.
 */
export function moduleListContentState(total: number, visible: number): ModuleListContentState {
  return total > 0 && visible === 0 ? "no-match" : "rows";
}

// ---------------------------------------------------------------- list context

/** A value the header shows and the body reads. */
export interface ModuleListContextValue {
  query: string;
  setQuery: (query: string) => void;
  sort: ModuleSort;
  filters: ModuleListFilters;
  /** True while any filter dimension carries a value. */
  filtersActive: boolean;
  /** Every module the header's search/filters narrow. */
  modules: ModuleRow[];
}
