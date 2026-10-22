// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import {
  clearViewListFilters,
  countActiveViewFilters,
  DEFAULT_VIEW_SORT,
  EMPTY_VIEW_LIST_FILTERS,
  filterProjectViews,
  hasActiveViewFilters,
  matchesOptionLabel,
  matchesViewFilters,
  matchesViewQuery,
  removeViewFilterDimension,
  removeViewFilterValue,
  selectViewFilterChipGroups,
  selectViewCustomCreated,
  selectViewSort,
  sortProjectViews,
  toggleViewAccessFilter,
  toggleViewCreatedPreset,
  toggleViewCreatorFilter,
  toggleViewFavoriteFilter,
  UNTITLED_VIEW_NAME,
  VIEW_ACCESS_OPTIONS,
  VIEW_ACCESS_PRIVATE,
  viewCreatorId,
  viewDisplayName,
  viewSortLabel,
} from "./viewListControls.js";

const NOW = Date.parse("2026-06-15T12:00:00Z");
const DAY = 86_400_000;

function isoDaysAgo(days: number): string {
  return new Date(NOW - days * DAY).toISOString();
}

describe("viewDisplayName and search (VIEW-004)", () => {
  it("shows a placeholder for a blank name", () => {
    expect(viewDisplayName("  ")).toBe(UNTITLED_VIEW_NAME);
    expect(viewDisplayName("Triage")).toBe("Triage");
  });

  it("matches by case-insensitive name substring and treats blank as everything", () => {
    const view = makeView({ name: "My Triage Board" });
    expect(matchesViewQuery(view, "")).toBe(true);
    expect(matchesViewQuery(view, "   ")).toBe(true);
    expect(matchesViewQuery(view, "triage")).toBe(true);
    expect(matchesViewQuery(view, "MY TRIAGE")).toBe(true);
    expect(matchesViewQuery(view, "beta")).toBe(false);
  });

  it("matches the placeholder for a blank name", () => {
    const view = makeView({ name: "" });
    expect(matchesViewQuery(view, "untitled")).toBe(true);
  });
});

describe("ordering (VIEW-005)", () => {
  const zebra = makeView({ name: "Zebra", created_at: "2026-01-03T00:00:00Z", updated_at: "2026-01-03T00:00:00Z" });
  const mango = makeView({ name: "mango", created_at: "2026-01-02T00:00:00Z", updated_at: "2026-01-04T00:00:00Z" });
  const apple = makeView({ name: "Apple", created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-02T00:00:00Z" });
  const rows = [zebra, mango, apple];

  it("defaults to updated-at descending and labels the key", () => {
    expect(DEFAULT_VIEW_SORT).toEqual({ key: "updated_at", direction: "descending" });
    expect(sortProjectViews(rows, DEFAULT_VIEW_SORT)).toEqual([mango, zebra, apple]);
    expect(viewSortLabel("name")).toBe("Name");
    expect(viewSortLabel("created_at")).toBe("Created at");
    expect(viewSortLabel("updated_at")).toBe("Updated at");
  });

  it("orders names case-insensitively in both directions", () => {
    expect(sortProjectViews(rows, { key: "name", direction: "ascending" }).map((v) => v.name)).toEqual([
      "Apple",
      "mango",
      "Zebra",
    ]);
    expect(sortProjectViews(rows, { key: "name", direction: "descending" }).map((v) => v.name)).toEqual([
      "Zebra",
      "mango",
      "Apple",
    ]);
  });

  it("orders creation and update stamps in both directions", () => {
    expect(sortProjectViews(rows, { key: "created_at", direction: "ascending" })).toEqual([apple, mango, zebra]);
    expect(sortProjectViews(rows, { key: "created_at", direction: "descending" })).toEqual([zebra, mango, apple]);
    expect(sortProjectViews(rows, { key: "updated_at", direction: "ascending" })).toEqual([apple, zebra, mango]);
  });

  it("treats names that differ only in case as equal", () => {
    const upper = makeView({ id: "a", name: "Alpha" });
    const lower = makeView({ id: "b", name: "alpha" });
    expect(sortProjectViews([upper, lower], { key: "name", direction: "descending" }).map((v) => v.id)).toEqual([
      "a",
      "b",
    ]);
  });

  it("never mutates the input and keeps ties stable", () => {
    const first = makeView({ id: "a", name: "Same", updated_at: "2026-01-01T00:00:00Z" });
    const second = makeView({ id: "b", name: "Same", updated_at: "2026-01-01T00:00:00Z" });
    const original = [first, second];
    expect(sortProjectViews(original, DEFAULT_VIEW_SORT).map((v) => v.id)).toEqual(["a", "b"]);
    expect(original[0]).toBe(first);
  });

  it("keeps the direction when the key changes and ignores a repeat pick", () => {
    const current = { key: "name", direction: "ascending" } as const;
    expect(selectViewSort(current, { key: "updated_at" })).toEqual({ key: "updated_at", direction: "ascending" });
    expect(selectViewSort(current, { direction: "descending" })).toEqual({ key: "name", direction: "descending" });
    expect(selectViewSort(current, { direction: "ascending" })).toBe(current);
    expect(selectViewSort(current, { key: "name" })).toBe(current);
  });
});

describe("filters (VIEW-006)", () => {
  const old = makeView({ created_at: isoDaysAgo(40), access: 1, is_favorite: true, created_by: "user-1" });
  const mid = makeView({ created_at: isoDaysAgo(10), access: 0, is_favorite: false, created_by: "user-2" });
  const fresh = makeView({
    created_at: isoDaysAgo(1),
    access: 1,
    is_favorite: false,
    created_by: null,
    owned_by: "user-2",
  });

  it("starts empty and toggles favorites", () => {
    expect(hasActiveViewFilters(EMPTY_VIEW_LIST_FILTERS)).toBe(false);
    const on = toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS);
    expect(on.favorites).toBe(true);
    expect(toggleViewFavoriteFilter(on).favorites).toBe(false);
    expect(matchesViewFilters(mid, on, NOW)).toBe(false);
    expect(matchesViewFilters(old, on, NOW)).toBe(true);
  });

  it("toggles access values and matches any selected value", () => {
    const publicOnly = toggleViewAccessFilter(EMPTY_VIEW_LIST_FILTERS, 1);
    expect(publicOnly.access).toEqual([1]);
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, publicOnly, NOW))).toEqual([old, fresh]);
    const both = toggleViewAccessFilter(publicOnly, 0);
    expect(both.access).toEqual([1, 0]);
    expect(toggleViewAccessFilter(both, 0).access).toEqual([1]);
    expect(VIEW_ACCESS_OPTIONS.map((option) => option.label)).toEqual(["Private", "Public"]);
  });

  it("treats presets as 'at least N days old' and combines them conjunctively", () => {
    const week = toggleViewCreatedPreset(EMPTY_VIEW_LIST_FILTERS, "1_week_ago");
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, week, NOW))).toEqual([old, mid]);
    const month = toggleViewCreatedPreset(week, "1_month_ago");
    expect(month.created.presets).toEqual(["1_week_ago", "1_month_ago"]);
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, month, NOW))).toEqual([old]);
    expect(toggleViewCreatedPreset(month, "1_week_ago").created.presets).toEqual(["1_month_ago"]);
  });

  it("ignores a created preset the options do not define", () => {
    const unknown = {
      ...EMPTY_VIEW_LIST_FILTERS,
      created: { presets: ["3_years_ago" as "1_week_ago"], custom: null },
    };
    expect(matchesViewFilters(old, unknown, NOW)).toBe(false);
  });

  it("keeps an unparseable stamp out of every preset", () => {
    const broken = makeView({ created_at: "not-a-date" });
    const week = toggleViewCreatedPreset(EMPTY_VIEW_LIST_FILTERS, "1_week_ago");
    expect(matchesViewFilters(broken, week, NOW)).toBe(false);
  });

  it("applies a custom window inclusively at both day ends", () => {
    const day = (value: Date): string => value.toISOString().slice(0, 10);
    const window = selectViewCustomCreated(EMPTY_VIEW_LIST_FILTERS, {
      start: day(new Date(NOW - 12 * DAY)),
      end: day(new Date(NOW - 5 * DAY)),
    });
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, window, NOW))).toEqual([mid]);
    const openEnd = selectViewCustomCreated(EMPTY_VIEW_LIST_FILTERS, {
      start: null,
      end: day(new Date(NOW - 20 * DAY)),
    });
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, openEnd, NOW))).toEqual([old]);
    expect(selectViewCustomCreated(window, null).created.custom).toBeNull();
  });

  it("matches creators by created_by and falls back to the owner", () => {
    expect(viewCreatorId(fresh)).toBe("user-2");
    const creators = toggleViewCreatorFilter(EMPTY_VIEW_LIST_FILTERS, "user-2");
    expect([old, mid, fresh].filter((v) => matchesViewFilters(v, creators, NOW))).toEqual([mid, fresh]);
    expect(toggleViewCreatorFilter(creators, "user-2").creators).toEqual([]);
  });

  it("counts every active dimension", () => {
    let filters = toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS);
    filters = toggleViewAccessFilter(filters, 1);
    filters = toggleViewCreatedPreset(filters, "2_weeks_ago");
    filters = selectViewCustomCreated(filters, { start: "2026-01-01", end: null });
    filters = toggleViewCreatorFilter(filters, "user-2");
    expect(countActiveViewFilters(filters)).toBe(5);
    expect(hasActiveViewFilters(filters)).toBe(true);
    expect(countActiveViewFilters(clearViewListFilters())).toBe(0);
  });

  it("composes search and filters, then leaves ordering to the caller", () => {
    const alpha = makeView({ id: "c", name: "Alpha", created_at: isoDaysAgo(40) });
    const beta = makeView({ id: "d", name: "Beta", created_at: isoDaysAgo(40) });
    const week = toggleViewCreatedPreset(EMPTY_VIEW_LIST_FILTERS, "1_week_ago");
    expect(filterProjectViews([alpha, beta], week, "alp", NOW)).toEqual([alpha]);
    expect(filterProjectViews([alpha, beta], EMPTY_VIEW_LIST_FILTERS, "", NOW)).toEqual([alpha, beta]);
  });

  it("matches option labels case-insensitively", () => {
    expect(matchesOptionLabel("Created date", "")).toBe(true);
    expect(matchesOptionLabel("Created date", " DATE ")).toBe(true);
    expect(matchesOptionLabel("Created date", "zzz")).toBe(false);
  });
});

describe("applied chips (VIEW-007)", () => {
  const members = [makeMember("user-1"), makeMember("user-2")];

  it("renders no group while nothing applies", () => {
    expect(selectViewFilterChipGroups(EMPTY_VIEW_LIST_FILTERS, members, "user-1")).toEqual([]);
  });

  it("renders one chip per dimension value in a fixed order", () => {
    let filters = toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS);
    filters = toggleViewAccessFilter(filters, 1);
    filters = toggleViewCreatedPreset(filters, "1_month_ago");
    filters = toggleViewCreatorFilter(filters, "user-2");
    const groups = selectViewFilterChipGroups(filters, members, "user-1");
    expect(groups.map((group) => group.dimension)).toEqual(["favorites", "access", "created", "creators"]);
    expect(groups[0]?.values).toEqual([{ value: "favorites", label: "Starred" }]);
    expect(groups[1]?.values).toEqual([{ value: "1", label: "Public" }]);
    expect(groups[2]?.values).toEqual([{ value: "1_month_ago", label: "1 month ago" }]);
    expect(groups[3]?.values).toEqual([
      { value: "user-2", label: "Ada Lovelace", avatarUrl: "https://cdn.example/ada.png" },
    ]);
  });

  it("omits the avatar when the member has none", () => {
    const bare = makeMember("user-3");
    bare.member.avatar_url = "";
    const filters = toggleViewCreatorFilter(EMPTY_VIEW_LIST_FILTERS, "user-3");
    expect(selectViewFilterChipGroups(filters, [bare], null)[0]?.values).toEqual([
      { value: "user-3", label: "Ada Lovelace" },
    ]);
  });

  it("drops an access value the filter does not offer", () => {
    const filters = { ...EMPTY_VIEW_LIST_FILTERS, access: [VIEW_ACCESS_PRIVATE, 7] };
    expect(selectViewFilterChipGroups(filters, members, null)[0]?.values).toEqual([{ value: "0", label: "Private" }]);
  });

  it("labels the caller You and shows the custom window", () => {
    let filters = toggleViewCreatorFilter(EMPTY_VIEW_LIST_FILTERS, VIEWS_OWNER_ID);
    filters = selectViewCustomCreated(filters, { start: "2026-01-01", end: "2026-01-31" });
    const groups = selectViewFilterChipGroups(filters, [makeMember(VIEWS_OWNER_ID)], VIEWS_OWNER_ID);
    expect(groups[0]?.values).toEqual([{ value: "custom", label: "Custom" }]);
    expect(groups[1]?.values[0]?.label).toBe("You");
  });

  it("falls back to the id for a creator missing from the roster", () => {
    const filters = toggleViewCreatorFilter(EMPTY_VIEW_LIST_FILTERS, "user-9");
    expect(selectViewFilterChipGroups(filters, members, null)[0]?.values).toEqual([
      { value: "user-9", label: "user-9" },
    ]);
    expect(selectViewFilterChipGroups(filters, undefined, null)[0]?.values).toEqual([
      { value: "user-9", label: "user-9" },
    ]);
  });

  it("removes one value and keeps every sibling filter", () => {
    let filters = toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS);
    filters = toggleViewCreatedPreset(filters, "1_week_ago");
    filters = toggleViewCreatedPreset(filters, "2_weeks_ago");
    const withoutWeek = removeViewFilterValue(filters, "created", "1_week_ago");
    expect(withoutWeek.favorites).toBe(true);
    expect(withoutWeek.created.presets).toEqual(["2_weeks_ago"]);
    expect(removeViewFilterValue(withoutWeek, "created", "2_weeks_ago").created.presets).toEqual([]);
  });

  it("removes a dimension whole and leaves siblings alone", () => {
    let filters = toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS);
    filters = toggleViewAccessFilter(filters, 0);
    filters = toggleViewCreatorFilter(filters, "user-2");
    const withoutAccess = removeViewFilterDimension(filters, "access");
    expect(withoutAccess).toEqual({ ...filters, access: [] });
    const withoutFavorites = removeViewFilterDimension(withoutAccess, "favorites");
    expect(withoutFavorites.creators).toEqual(["user-2"]);
    const withoutCreated = removeViewFilterDimension(
      { ...withoutFavorites, created: { presets: ["1_week_ago"], custom: { start: "2026-01-01", end: null } } },
      "created"
    );
    expect(withoutCreated.created).toEqual({ presets: [], custom: null });
    expect(removeViewFilterDimension(withoutCreated, "creators").creators).toEqual([]);
  });

  it("removes the custom value without touching presets", () => {
    const filters = selectViewCustomCreated(toggleViewCreatedPreset(EMPTY_VIEW_LIST_FILTERS, "1_week_ago"), {
      start: "2026-01-01",
      end: null,
    });
    const removed = removeViewFilterValue(filters, "created", "custom");
    expect(removed.created).toEqual({ presets: ["1_week_ago"], custom: null });
  });

  it("removing a favorites value clears only that dimension", () => {
    const filters = toggleViewAccessFilter(toggleViewFavoriteFilter(EMPTY_VIEW_LIST_FILTERS), 0);
    const removed = removeViewFilterValue(filters, "favorites", "favorites");
    expect(removed.favorites).toBe(false);
    expect(removed.access).toEqual([0]);
  });

  it("removing a creator keeps the other creators", () => {
    let filters = toggleViewCreatorFilter(EMPTY_VIEW_LIST_FILTERS, "user-1");
    filters = toggleViewCreatorFilter(filters, "user-2");
    expect(removeViewFilterValue(filters, "creators", "user-1").creators).toEqual(["user-2"]);
  });

  it("removing an access value keeps the other access values", () => {
    let filters = toggleViewAccessFilter(EMPTY_VIEW_LIST_FILTERS, 0);
    filters = toggleViewAccessFilter(filters, 1);
    expect(removeViewFilterValue(filters, "access", "0").access).toEqual([1]);
  });
});
