// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages list model (parity/specs/pages.md §2.6, §3.1–§3.5, PAGE-002–005).
// Everything the list decides before it renders lives here: the tab in
// the address, the one client-side collection partition, the sort, the
// title search, the structured filters and their chip labels. Pure, so
// the route, the components and the tests share exactly one reading.

import type { PageRow } from "@pidash/api-client";
import { PAGE_LIST_TABS, parsePageListSearch, type PageListSearch, type PageListTab } from "./pageListSearch.js";

export { PAGE_LIST_TABS, parsePageListSearch };
export type { PageListSearch, PageListTab };

/** Whether the row belongs in one tab. Archive wins over access. */
export function pageInTab(row: Pick<PageRow, "access" | "archived_at">, tab: PageListTab): boolean {
  if (tab === "archived") return row.archived_at !== null;
  if (row.archived_at !== null) return false;
  return tab === "private" ? row.access === 1 : row.access === 0;
}

/** Visible label for a page; blank names read as untitled. */
export const PAGE_UNTITLED = "Untitled";

export function pageDisplayTitle(name: string): string {
  return name.trim() === "" ? PAGE_UNTITLED : name;
}

/** State key the address keeps for a tab. */
export function pageTabHref(workspaceSlug: string, projectId: string, tab: PageListTab): string {
  return `/${workspaceSlug}/projects/${projectId}/pages?type=${tab}`;
}

/** Plain collection address with the trailing slash the recovery link keeps (PAGE-016). */
export function pageListHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/pages/`;
}

/** Canonical detail address a row opens. */
export function pageDetailHref(workspaceSlug: string, projectId: string, pageId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/pages/${pageId}`;
}

/** Feature-settings leaf the gate's admin shortcut opens. */
export function pagesFeatureHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/features/pages`;
}

export type PageSortKey = "name" | "created" | "modified";
export type PageSortDirection = "ascending" | "descending";

export interface PageSort {
  key: PageSortKey;
  direction: PageSortDirection;
}

/** Newest modification first: the list's first load and reset order. */
export const DEFAULT_PAGE_SORT: PageSort = { key: "modified", direction: "descending" };

export const PAGE_SORT_KEYS: ReadonlyArray<{ key: PageSortKey; label: string }> = [
  { key: "name", label: "Name" },
  { key: "created", label: "Date created" },
  { key: "modified", label: "Date modified" },
];

export const PAGE_SORT_DIRECTIONS: ReadonlyArray<{ direction: PageSortDirection; label: string }> = [
  { direction: "ascending", label: "Ascending" },
  { direction: "descending", label: "Descending" },
];

export function pageSortKeyLabel(key: PageSortKey): string {
  return PAGE_SORT_KEYS.find((option) => option.key === key)?.label ?? "Date modified";
}

/** Locale-aware, case-insensitive name comparison; untitled uses its label. */
const titleCollator = new Intl.Collator(undefined, { sensitivity: "base" });

export function comparePageTitles(left: string, right: string): number {
  return titleCollator.compare(pageDisplayTitle(left), pageDisplayTitle(right));
}

/** Stable client sort: ties keep collection order, then direction flips. */
export function sortPageRows(rows: readonly PageRow[], sort: PageSort): PageRow[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  indexed.sort((left, right) => {
    let verdict: number;
    if (sort.key === "name") {
      verdict = comparePageTitles(left.row.name, right.row.name);
    } else if (sort.key === "created") {
      verdict = left.row.created_at < right.row.created_at ? -1 : left.row.created_at > right.row.created_at ? 1 : 0;
    } else {
      verdict = left.row.updated_at < right.row.updated_at ? -1 : left.row.updated_at > right.row.updated_at ? 1 : 0;
    }
    return verdict === 0 ? left.index - right.index : verdict;
  });
  const ordered = indexed.map((entry) => entry.row);
  if (sort.direction === "descending") ordered.reverse();
  return ordered;
}

/** Sort one sibling list by the active sort, keeping favourites first. */
export function sortPageSiblings(rows: readonly PageRow[], sort: PageSort): PageRow[] {
  const favourites = sortPageRows(
    rows.filter((row) => row.is_favorite),
    sort
  );
  const rest = sortPageRows(
    rows.filter((row) => !row.is_favorite),
    sort
  );
  return [...favourites, ...rest];
}

/** Title search: case-insensitive substring over the visible label. */
export function matchesPageSearch(row: Pick<PageRow, "name">, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return pageDisplayTitle(row.name).toLowerCase().includes(needle);
}

export type PageDatePreset =
  | "today"
  | "yesterday"
  | "last_7_days"
  | "last_30_days"
  | "1_week_ago"
  | "2_weeks_ago"
  | "1_month_ago"
  | "1_week_from_now"
  | "2_weeks_from_now"
  | "1_month_from_now";

/**
 * The creation condition is either one of the relative presets or an
 * explicit calendar day in `YYYY-MM-DD` form.
 */
export type PageDateCondition = PageDatePreset | { custom: string };

export const PAGE_DATE_PRESETS: ReadonlyArray<{ value: PageDatePreset; label: string }> = [
  { value: "today", label: "Today" },
  { value: "yesterday", label: "Yesterday" },
  { value: "last_7_days", label: "Last 7 days" },
  { value: "last_30_days", label: "Last 30 days" },
  { value: "1_week_ago", label: "1 week ago" },
  { value: "2_weeks_ago", label: "2 weeks ago" },
  { value: "1_month_ago", label: "1 month ago" },
  { value: "1_week_from_now", label: "1 week from now" },
  { value: "2_weeks_from_now", label: "2 weeks from now" },
  { value: "1_month_from_now", label: "1 month from now" },
];

export interface PageFilters {
  favorites: boolean;
  created: PageDateCondition | null;
  /** Owning user id, or null when no creator filter is applied. */
  creator: string | null;
}

export const EMPTY_PAGE_FILTERS: PageFilters = { favorites: false, created: null, creator: null };

export function hasPageFilters(filters: PageFilters): boolean {
  return filters.favorites || filters.created !== null || filters.creator !== null;
}

/** Start of the local calendar day `offsetDays` away from `now`. */
function dayStart(now: Date, offsetDays = 0): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + offsetDays).getTime();
}

/**
 * Evaluate a relative creation-date condition at a supplied instant.
 * Every preset is a whole-calendar-day offset from the local start of today,
 * matching the server expression the old app sent: "last N days" keeps N days
 * back inclusive, "N ago" keeps everything older than that boundary, and
 * "N from now" starts N days ahead.
 */
export function matchesCreationPreset(createdAt: string, condition: PageDateCondition, now: Date): boolean {
  const created = Date.parse(createdAt);
  if (Number.isNaN(created)) return false;
  if (typeof condition === "object") return sameLocalDay(new Date(created), condition.custom);
  switch (condition) {
    case "today":
      return created >= dayStart(now) && created < dayStart(now, 1);
    case "yesterday":
      return created >= dayStart(now, -1) && created < dayStart(now);
    case "last_7_days":
      return created >= dayStart(now, -7);
    case "last_30_days":
      return created >= dayStart(now, -30);
    case "1_week_ago":
      return created < dayStart(now, -6);
    case "2_weeks_ago":
      return created < dayStart(now, -13);
    case "1_month_ago":
      return created < dayStart(now, -29);
    case "1_week_from_now":
      return created >= dayStart(now, 7);
    case "2_weeks_from_now":
      return created >= dayStart(now, 14);
    case "1_month_from_now":
      return created >= dayStart(now, 30);
  }
}

/** Whether an instant falls on the `YYYY-MM-DD` calendar day (local time). */
function sameLocalDay(date: Date, isoDay: string): boolean {
  const parts = /^(\d{4})-(\d{2})-(\d{2})$/.exec(isoDay);
  if (parts === null) return false;
  const [, year, month, day] = parts;
  return date.getFullYear() === Number(year) && date.getMonth() === Number(month) - 1 && date.getDate() === Number(day);
}

/** Apply every active dimension; dimensions combine with AND. */
export function matchesPageFilters(
  row: Pick<PageRow, "created_at" | "is_favorite" | "owned_by">,
  filters: PageFilters,
  now: Date
): boolean {
  if (filters.favorites && !row.is_favorite) return false;
  if (filters.created !== null && !matchesCreationPreset(row.created_at, filters.created, now)) return false;
  if (filters.creator !== null && row.owned_by !== filters.creator) return false;
  return true;
}

/** The tab partition, then sort, then search, then filters. */
export function visiblePageRows(
  rows: readonly PageRow[],
  tab: PageListTab,
  sort: PageSort,
  query: string,
  filters: PageFilters,
  now: Date
): PageRow[] {
  const tabbed = rows.filter((row) => pageInTab(row, tab));
  return sortPageRows(tabbed, sort).filter(
    (row) => matchesPageSearch(row, query) && matchesPageFilters(row, filters, now)
  );
}

export type PageChipId = "favorites" | "created" | "creator";

/** Applied filters in display order, with the labels the strip renders. */
export interface PageFilterChip {
  id: PageChipId;
  label: string;
  value: string;
}

export function pageFilterChips(filters: PageFilters, creatorName: string | null): PageFilterChip[] {
  const chips: PageFilterChip[] = [];
  if (filters.favorites) chips.push({ id: "favorites", label: "Favorites", value: "Favorites" });
  if (filters.created !== null) {
    const preset = PAGE_DATE_PRESETS.find((option) => option.value === filters.created);
    const value =
      typeof filters.created === "object"
        ? formatCustomDate(filters.created.custom)
        : (preset?.label ?? filters.created);
    chips.push({ id: "created", label: "Created At", value });
  }
  if (filters.creator !== null) {
    chips.push({ id: "creator", label: "Created By", value: creatorName ?? "Unknown member" });
  }
  return chips;
}

/** Human label for the custom calendar option; the raw value survives bad input. */
export function formatCustomDate(isoDay: string): string {
  const parsed = /^(\d{4})-(\d{2})-(\d{2})$/.exec(isoDay);
  if (parsed === null) return isoDay;
  const date = new Date(Number(parsed[1]), Number(parsed[2]) - 1, Number(parsed[3]));
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

/** Remove one dimension and leave every other applied filter intact. */
export function removePageFilter(filters: PageFilters, id: PageChipId): PageFilters {
  if (id === "favorites") return { ...filters, favorites: false };
  if (id === "created") return { ...filters, created: null };
  return { ...filters, creator: null };
}

/** Member option behind the creator picker. */
export interface PageCreatorOption {
  id: string;
  name: string;
  avatarUrl: string | null;
}

/** Signed-in user id, or null until the session read resolves. */
export function pageSessionUserId(me: { id: string } | undefined): string | null {
  return me === undefined ? null : me.id;
}

/** Sort options by visible name and narrow them with the panel search. */
export function selectPageCreators(
  creators: readonly PageCreatorOption[],
  query: string,
  currentUserId: string | null
): PageCreatorOption[] {
  const needle = query.trim().toLowerCase();
  return creators
    .map((creator) => (creator.id === currentUserId ? { ...creator, name: "You" } : creator))
    .filter((creator) => needle === "" || creator.name.toLowerCase().includes(needle))
    .sort((left, right) => titleCollator.compare(left.name, right.name));
}

/** Visible creator name for a filter id, or null when the member is not in the directory. */
export function pageCreatorName(creators: readonly PageCreatorOption[], creatorId: string | null): string | null {
  if (creatorId === null) return null;
  return creators.find((creator) => creator.id === creatorId)?.name ?? null;
}
