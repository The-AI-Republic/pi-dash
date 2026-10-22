// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list controls (parity/specs/views.md §4, VIEW-004-007).
// Pure model behind the header's search, sort and filter menus and the
// applied-chip strip: name matching, ordering, the four filter dimensions,
// chip descriptors and the single-value removal rules. Everything here is
// computed on the fetched list; none of it writes to the server.

import type { SavedView, WorkspaceMember } from "@pidash/api-client";

import type { RangeValue } from "../../issues/index.js";
import { VIEW_ACCESS_SHARED, viewAccessLabel } from "./projectViews.js";

// --- display name and search (VIEW-004) -------------------------------------

/** Placeholder shown and matched for a record with no name. */
export const UNTITLED_VIEW_NAME = "Untitled";

/** The name a row shows: the stored name, or the placeholder when blank. */
export function viewDisplayName(name: string): string {
  return name.trim() === "" ? UNTITLED_VIEW_NAME : name;
}

/** Edition flag that mounts the cloud-only access dimension (VIEW-006). */
export const VIEW_ACCESS_FILTER_FLAG = "view-access-filter";

/** True where the build offers the access dimension; the OSS build does not. */
export function isViewAccessFilterEnabled(build: { flags: Record<string, boolean> }): boolean {
  return build.flags[VIEW_ACCESS_FILTER_FLAG] === true;
}

/** Case-insensitive substring match over the display name. */
export function matchesViewQuery(view: SavedView, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return viewDisplayName(view.name).toLowerCase().includes(needle);
}

// --- ordering (VIEW-005) ----------------------------------------------------

export type ViewSortKey = "name" | "created_at" | "updated_at";
export type ViewSortDirection = "ascending" | "descending";

export interface ViewSort {
  key: ViewSortKey;
  direction: ViewSortDirection;
}

/** Reload default: newest update first (spec §4). */
export const DEFAULT_VIEW_SORT: ViewSort = { key: "updated_at", direction: "descending" };

export const VIEW_SORT_KEY_OPTIONS: ReadonlyArray<{ key: ViewSortKey; label: string }> = [
  { key: "name", label: "Name" },
  { key: "created_at", label: "Created at" },
  { key: "updated_at", label: "Updated at" },
];

export const VIEW_SORT_DIRECTION_OPTIONS: ReadonlyArray<{ direction: ViewSortDirection; label: string }> = [
  { direction: "ascending", label: "Ascending" },
  { direction: "descending", label: "Descending" },
];

/** Trigger label: the active key, never the direction. */
export function viewSortLabel(key: ViewSortKey): string {
  return VIEW_SORT_KEY_OPTIONS.find((option) => option.key === key)?.label as string;
}

/**
 * Picking a key keeps the current direction; re-picking the active key or
 * direction is a no-op and returns the same object so callers can skip a
 * state write.
 */
export function selectViewSort(sort: ViewSort, choice: Partial<ViewSort>): ViewSort {
  const key = choice.key ?? sort.key;
  const direction = choice.direction ?? sort.direction;
  if (key === sort.key && direction === sort.direction) return sort;
  return { key, direction };
}

function compareNames(left: string, right: string): number {
  const a = viewDisplayName(left).toLowerCase();
  const b = viewDisplayName(right).toLowerCase();
  if (a === b) return 0;
  return a < b ? -1 : 1;
}

function compareViews(left: SavedView, right: SavedView, key: ViewSortKey): number {
  if (key === "name") return compareNames(left.name, right.name);
  const a = key === "created_at" ? left.created_at : left.updated_at;
  const b = key === "created_at" ? right.created_at : right.updated_at;
  // ISO-8601 stamps with the same offset order lexicographically.
  return a === b ? 0 : a < b ? -1 : 1;
}

/** Stable order for the display; never mutates the input. */
export function sortProjectViews(views: readonly SavedView[], sort: ViewSort): SavedView[] {
  const sign = sort.direction === "ascending" ? 1 : -1;
  return [...views].sort((left, right) => sign * compareViews(left, right, sort.key));
}

// --- filters (VIEW-006) -----------------------------------------------------

export type ViewCreatedPreset = "1_week_ago" | "2_weeks_ago" | "1_month_ago";

export interface ViewCreatedPresetOption {
  value: ViewCreatedPreset;
  label: string;
  /** Minimum age in whole days before "now". */
  days: number;
}

export const VIEW_CREATED_PRESETS: readonly ViewCreatedPresetOption[] = [
  { value: "1_week_ago", label: "1 week ago", days: 7 },
  { value: "2_weeks_ago", label: "2 weeks ago", days: 14 },
  { value: "1_month_ago", label: "1 month ago", days: 30 },
];

export const VIEW_CREATED_CUSTOM_VALUE = "custom";
export const VIEW_CREATED_CUSTOM_LABEL = "Custom";

/**
 * Access values the filter offers. Labels are the row badges' own
 * (VIEW-011): the shared value and everything else read owner-only.
 */
export const VIEW_ACCESS_PRIVATE = 0;

export const VIEW_ACCESS_OPTIONS: ReadonlyArray<{ value: number; label: string }> = [
  { value: VIEW_ACCESS_PRIVATE, label: viewAccessLabel(VIEW_ACCESS_PRIVATE) },
  { value: VIEW_ACCESS_SHARED, label: viewAccessLabel(VIEW_ACCESS_SHARED) },
];

export interface ViewCreatedFilter {
  /** Relative presets; every selected preset must hold (conjunctive). */
  presets: readonly ViewCreatedPreset[];
  /** Absolute window; inclusive at both day ends. */
  custom: RangeValue | null;
}

export interface ViewListFilters {
  /** Starred rows only. */
  favorites: boolean;
  /** Access values; the dimension is only rendered where the edition offers it. */
  access: readonly number[];
  created: ViewCreatedFilter;
  /** Creator user ids. */
  creators: readonly string[];
}

export const EMPTY_VIEW_LIST_FILTERS: ViewListFilters = {
  favorites: false,
  access: [],
  created: { presets: [], custom: null },
  creators: [],
};

const DAY_MS = 86_400_000;

function ageInMs(createdAt: string, now: number): number {
  const stamp = Date.parse(createdAt);
  return Number.isNaN(stamp) ? Number.NaN : now - stamp;
}

function matchesPreset(createdAt: string, preset: ViewCreatedPreset, now: number): boolean {
  const option = VIEW_CREATED_PRESETS.find((entry) => entry.value === preset);
  if (option === undefined) return false;
  const age = ageInMs(createdAt, now);
  // "At least N days old", not "in the last N days" (spec §4).
  return !Number.isNaN(age) && age >= option.days * DAY_MS;
}

function matchesCustomRange(createdAt: string, range: RangeValue): boolean {
  const day = createdAt.slice(0, 10);
  if (range.start !== null && day < range.start) return false;
  if (range.end !== null && day > range.end) return false;
  return true;
}

/**
 * The person a view belongs to for the Created by dimension: the recorded
 * author, falling back to the owner when the record predates the field.
 */
export function viewCreatorId(view: SavedView): string {
  return view.created_by ?? view.owned_by;
}

/** Every active dimension must hold; presets are conjunctive within theirs. */
export function matchesViewFilters(view: SavedView, filters: ViewListFilters, now: number): boolean {
  if (filters.favorites && view.is_favorite !== true) return false;
  if (filters.access.length > 0 && !filters.access.includes(view.access)) return false;
  for (const preset of filters.created.presets) {
    if (!matchesPreset(view.created_at, preset, now)) return false;
  }
  if (filters.created.custom !== null && !matchesCustomRange(view.created_at, filters.created.custom)) return false;
  if (filters.creators.length > 0) {
    if (!filters.creators.includes(viewCreatorId(view))) return false;
  }
  return true;
}

/** Search and filters compose; the caller sorts the result. */
export function filterProjectViews(
  views: readonly SavedView[],
  filters: ViewListFilters,
  query: string,
  now: number
): SavedView[] {
  return views.filter((view) => matchesViewQuery(view, query) && matchesViewFilters(view, filters, now));
}

export function countActiveViewFilters(filters: ViewListFilters): number {
  return (
    (filters.favorites ? 1 : 0) +
    filters.access.length +
    filters.created.presets.length +
    (filters.created.custom === null ? 0 : 1) +
    filters.creators.length
  );
}

/** True while at least one filter dimension narrows the list. */
export function hasActiveViewFilters(filters: ViewListFilters): boolean {
  return countActiveViewFilters(filters) > 0;
}

export function toggleViewFavoriteFilter(filters: ViewListFilters): ViewListFilters {
  return { ...filters, favorites: !filters.favorites };
}

function toggleValue<T>(values: readonly T[], value: T): T[] {
  return values.includes(value) ? values.filter((entry) => entry !== value) : [...values, value];
}

export function toggleViewAccessFilter(filters: ViewListFilters, access: number): ViewListFilters {
  return { ...filters, access: toggleValue(filters.access, access) };
}

/** Selecting one preset adds it; selecting it again drops it. */
export function toggleViewCreatedPreset(filters: ViewListFilters, preset: ViewCreatedPreset): ViewListFilters {
  return { ...filters, created: { ...filters.created, presets: toggleValue(filters.created.presets, preset) } };
}

/** The custom window; a null window clears the "Custom" chip. */
export function selectViewCustomCreated(filters: ViewListFilters, range: RangeValue | null): ViewListFilters {
  return { ...filters, created: { ...filters.created, custom: range } };
}

export function toggleViewCreatorFilter(filters: ViewListFilters, creatorId: string): ViewListFilters {
  return { ...filters, creators: toggleValue(filters.creators, creatorId) };
}

// --- applied chips (VIEW-007) ----------------------------------------------

export type ViewFilterDimension = "favorites" | "access" | "created" | "creators";

export interface ViewFilterChipValue {
  /** Removal token within the dimension. */
  value: string;
  label: string;
  /** Optional avatar for a person chip. */
  avatarUrl?: string | undefined;
}

export interface ViewFilterChipGroup {
  dimension: ViewFilterDimension;
  label: string;
  values: ViewFilterChipValue[];
}

/** The single value chip the Favorites dimension shows (the switch is on). */
export const VIEW_FAVORITES_CHIP_LABEL = "Starred";

export const VIEW_FILTER_DIMENSION_LABELS: Record<ViewFilterDimension, string> = {
  favorites: "Favorites",
  access: "Access",
  created: "Created at",
  creators: "Created by",
};

function accessLabel(value: number): string | undefined {
  return VIEW_ACCESS_OPTIONS.find((option) => option.value === value)?.label;
}

function createdLabels(created: ViewCreatedFilter): ViewFilterChipValue[] {
  const chips: ViewFilterChipValue[] = created.presets.map((preset) => ({
    value: preset,
    label: VIEW_CREATED_PRESETS.find((option) => option.value === preset)?.label as string,
  }));
  if (created.custom !== null) chips.push({ value: VIEW_CREATED_CUSTOM_VALUE, label: VIEW_CREATED_CUSTOM_LABEL });
  return chips;
}

function creatorChips(
  creators: readonly string[],
  members: readonly WorkspaceMember[] | undefined,
  selfId: string | null
): ViewFilterChipValue[] {
  // Filter values and roster ids compare as strings: a UUID string is the
  // same creator however the caller spelled it.
  return creators.flatMap((rawId) => {
    const id = `${rawId}`;
    const member = members?.find((entry) => entry.member.id === id)?.member;
    // A creator who left the roster still narrows the list; the chip shows
    // the raw id rather than dropping the dimension silently.
    if (member === undefined) return [{ value: id, label: id }];
    return [
      {
        value: id,
        label: selfId !== null && id === selfId ? "You" : member.display_name,
        avatarUrl: member.avatar_url === "" || member.avatar_url === null ? undefined : member.avatar_url,
      },
    ];
  });
}

/** One group per active dimension, in the strip's fixed order. */
export function selectViewFilterChipGroups(
  filters: ViewListFilters,
  members: readonly WorkspaceMember[] | undefined,
  selfId: string | null
): ViewFilterChipGroup[] {
  const groups: ViewFilterChipGroup[] = [];
  if (filters.favorites) {
    groups.push({
      dimension: "favorites",
      label: VIEW_FILTER_DIMENSION_LABELS.favorites,
      values: [{ value: "favorites", label: VIEW_FAVORITES_CHIP_LABEL }],
    });
  }
  if (filters.access.length > 0) {
    groups.push({
      dimension: "access",
      label: VIEW_FILTER_DIMENSION_LABELS.access,
      values: filters.access.flatMap((value) => {
        const label = accessLabel(value);
        return label === undefined ? [] : [{ value: `${value}`, label }];
      }),
    });
  }
  const created = createdLabels(filters.created);
  if (created.length > 0)
    groups.push({ dimension: "created", label: VIEW_FILTER_DIMENSION_LABELS.created, values: created });
  const creators = creatorChips(filters.creators, members, selfId);
  if (creators.length > 0) {
    groups.push({ dimension: "creators", label: VIEW_FILTER_DIMENSION_LABELS.creators, values: creators });
  }
  return groups;
}

/** Removing a value edits only that value; siblings always survive. */
export function removeViewFilterValue(
  filters: ViewListFilters,
  dimension: ViewFilterDimension,
  value: string
): ViewListFilters {
  switch (dimension) {
    case "favorites":
      return { ...filters, favorites: false };
    case "access":
      return { ...filters, access: filters.access.filter((entry) => `${entry}` !== value) };
    case "created":
      if (value === VIEW_CREATED_CUSTOM_VALUE) return selectViewCustomCreated(filters, null);
      return {
        ...filters,
        created: {
          ...filters.created,
          presets: filters.created.presets.filter((entry) => entry !== value),
        },
      };
    case "creators":
      return { ...filters, creators: filters.creators.filter((entry) => entry !== value) };
  }
}

/** Removing a dimension clears it whole; every other dimension survives. */
export function removeViewFilterDimension(filters: ViewListFilters, dimension: ViewFilterDimension): ViewListFilters {
  switch (dimension) {
    case "favorites":
      return { ...filters, favorites: false };
    case "access":
      return { ...filters, access: [] };
    case "created":
      return { ...filters, created: EMPTY_VIEW_LIST_FILTERS.created };
    case "creators":
      return { ...filters, creators: [] };
  }
}

export function clearViewListFilters(): ViewListFilters {
  return EMPTY_VIEW_LIST_FILTERS;
}

// --- filter menu options ----------------------------------------------------

/** Case-insensitive substring match used by the menu's own option search. */
export function matchesOptionLabel(label: string, query: string): boolean {
  const needle = query.trim().toLowerCase();
  return needle === "" || label.toLowerCase().includes(needle);
}
