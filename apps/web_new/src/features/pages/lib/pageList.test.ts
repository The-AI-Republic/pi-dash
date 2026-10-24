// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure pages list rules: PAGE-002 tabs and URL, PAGE-003 search,
// PAGE-004 sort, PAGE-005 filters and chips.

import { describe, expect, it } from "vitest";

import { PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import {
  EMPTY_PAGE_FILTERS,
  PAGE_DATE_PRESETS,
  PAGE_UNTITLED,
  comparePageTitles,
  hasPageFilters,
  matchesCreationPreset,
  matchesPageFilters,
  matchesPageSearch,
  formatCustomDate,
  pageDisplayTitle,
  pageFilterChips,
  pageCreatorName,
  pageSessionUserId,
  pageInTab,
  pageSortKeyLabel,
  pageTabHref,
  pagesFeatureHref,
  parsePageListSearch,
  removePageFilter,
  selectPageCreators,
  sortPageRows,
  visiblePageRows,
  DEFAULT_PAGE_SORT,
  type PageDatePreset,
} from "./pageList.js";

const NOW = new Date("2026-10-23T12:00:00Z");

describe("parsePageListSearch and pageTabHref", () => {
  it("lands missing, unknown and repeated values on public", () => {
    expect(parsePageListSearch({})).toEqual({ type: "public" });
    expect(parsePageListSearch({ type: "bogus" })).toEqual({ type: "public" });
    expect(parsePageListSearch({ type: ["private"] })).toEqual({ type: "private" });
  });

  it("keeps the two non-default tabs", () => {
    expect(parsePageListSearch({ type: "private" })).toEqual({ type: "private" });
    expect(parsePageListSearch({ type: "archived" })).toEqual({ type: "archived" });
  });

  it("writes the shareable query and feature leaf", () => {
    expect(pageTabHref("acme", PAGES_PROJECT_ID, "private")).toBe(
      `/acme/projects/${PAGES_PROJECT_ID}/pages?type=private`
    );
    expect(pagesFeatureHref("acme", PAGES_PROJECT_ID)).toBe(
      `/acme/settings/projects/${PAGES_PROJECT_ID}/features/pages`
    );
  });
});

describe("pageInTab", () => {
  it("partitions open, restricted and archived rows", () => {
    expect(pageInTab(makePageRow({ access: 0 }), "public")).toBe(true);
    expect(pageInTab(makePageRow({ access: 1 }), "public")).toBe(false);
    expect(pageInTab(makePageRow({ access: 1 }), "private")).toBe(true);
    expect(pageInTab(makePageRow({ access: 0 }), "private")).toBe(false);
    expect(pageInTab(makePageRow({ archived_at: "2026-10-01T00:00:00Z" }), "archived")).toBe(true);
    expect(pageInTab(makePageRow({ access: 1, archived_at: "2026-10-01T00:00:00Z" }), "archived")).toBe(true);
    expect(pageInTab(makePageRow({ archived_at: "2026-10-01T00:00:00Z" }), "public")).toBe(false);
  });
});

describe("display and search", () => {
  it("uses Untitled for a blank title, including whitespace", () => {
    expect(pageDisplayTitle("  ")).toBe(PAGE_UNTITLED);
    expect(pageDisplayTitle("Plan")).toBe("Plan");
  });

  it("matches titles case-insensitively and matches empty queries", () => {
    expect(matchesPageSearch({ name: "Alpha Plan" }, "ALP")).toBe(true);
    expect(matchesPageSearch({ name: "Alpha Plan" }, "beta")).toBe(false);
    expect(matchesPageSearch({ name: "Alpha Plan" }, "  ")).toBe(true);
    expect(matchesPageSearch({ name: " " }, "untitled")).toBe(true);
  });

  it("compares names case-insensitively", () => {
    expect(comparePageTitles("alpha", "Beta")).toBeLessThan(0);
    expect(comparePageTitles("Same", "same")).toBe(0);
  });
});

describe("sortPageRows", () => {
  const older = makePageRow({
    id: "a",
    name: "Charlie",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-03-02T00:00:00Z",
  });
  const newer = makePageRow({
    id: "b",
    name: "alpha",
    created_at: "2026-02-01T00:00:00Z",
    updated_at: "2026-03-01T00:00:00Z",
  });
  const blank = makePageRow({
    id: "c",
    name: " ",
    created_at: "2026-03-01T00:00:00Z",
    updated_at: "2026-03-03T00:00:00Z",
  });
  const rows = [older, newer, blank];

  it("defaults to modified descending", () => {
    expect(DEFAULT_PAGE_SORT).toEqual({ key: "modified", direction: "descending" });
    expect(sortPageRows(rows, DEFAULT_PAGE_SORT).map((row) => row.id)).toEqual(["c", "a", "b"]);
  });

  it("sorts by name in both directions using the fallback title", () => {
    expect(sortPageRows(rows, { key: "name", direction: "ascending" }).map((row) => row.id)).toEqual(["b", "a", "c"]);
    expect(sortPageRows(rows, { key: "name", direction: "descending" }).map((row) => row.id)).toEqual(["c", "a", "b"]);
  });

  it("compares numbered titles lexicographically rather than numerically", () => {
    const ten = makePageRow({ id: "ten", name: "Page 10" });
    const two = makePageRow({ id: "two", name: "Page 2" });
    expect(sortPageRows([two, ten], { key: "name", direction: "ascending" }).map((row) => row.id)).toEqual([
      "ten",
      "two",
    ]);
  });

  it("sorts by creation in both directions", () => {
    expect(sortPageRows(rows, { key: "created", direction: "ascending" }).map((row) => row.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
    expect(sortPageRows(rows, { key: "created", direction: "descending" }).map((row) => row.id)).toEqual([
      "c",
      "b",
      "a",
    ]);
    expect(sortPageRows([...rows].reverse(), { key: "created", direction: "ascending" }).map((row) => row.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("keeps collection order for equal creation and modification stamps", () => {
    const first = makePageRow({ id: "first", created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-01T00:00:00Z" });
    const second = makePageRow({
      id: "second",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    expect(sortPageRows([first, second], { key: "created", direction: "ascending" }).map((row) => row.id)).toEqual([
      "first",
      "second",
    ]);
    expect(sortPageRows([first, second], { key: "modified", direction: "ascending" }).map((row) => row.id)).toEqual([
      "first",
      "second",
    ]);
  });

  it("keeps stable ties", () => {
    const first = makePageRow({ id: "first", name: "same", updated_at: "2026-01-01T00:00:00Z" });
    const second = makePageRow({ id: "second", name: "same", updated_at: "2026-01-01T00:00:00Z" });
    expect(sortPageRows([first, second], { key: "name", direction: "ascending" }).map((row) => row.id)).toEqual([
      "first",
      "second",
    ]);
  });

  it("falls back to the modified label for an unknown key", () => {
    expect(pageSortKeyLabel("modified")).toBe("Date modified");
    expect(pageSortKeyLabel("bogus" as never)).toBe("Date modified");
  });
});

describe("creation filters", () => {
  const localStartOfToday = new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate()).getTime();
  const localStartDaysAgo = (days: number): number =>
    new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate() - days).getTime();
  /** An instant on the local calendar day `offsetDays` away from today. */
  const atDay = (offsetDays: number, hour = 0): string =>
    new Date(
      new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate() + offsetDays).getTime() + hour * 3_600_000
    ).toISOString();

  const cases: ReadonlyArray<[PageDatePreset, string, boolean]> = [
    ["today", atDay(0, 9), true],
    ["today", atDay(-1, 9), false],
    ["yesterday", atDay(-1, 9), true],
    ["yesterday", atDay(0, 9), false],
    ["last_7_days", atDay(-7), true],
    ["last_7_days", new Date(Date.parse(atDay(-7)) - 1).toISOString(), false],
    ["last_30_days", atDay(-30), true],
    ["last_30_days", new Date(Date.parse(atDay(-30)) - 1).toISOString(), false],
    ["1_week_ago", atDay(-7), true],
    ["1_week_ago", atDay(-90), true],
    ["1_week_ago", atDay(-6), false],
    ["2_weeks_ago", atDay(-14), true],
    ["2_weeks_ago", atDay(-90), true],
    ["2_weeks_ago", atDay(-13), false],
    ["1_month_ago", atDay(-30), true],
    ["1_month_ago", atDay(-90), true],
    ["1_month_ago", atDay(-29), false],
    ["1_week_from_now", atDay(7), true],
    ["1_week_from_now", new Date(Date.parse(atDay(7)) - 1).toISOString(), false],
    ["2_weeks_from_now", atDay(14), true],
    ["2_weeks_from_now", new Date(Date.parse(atDay(14)) - 1).toISOString(), false],
    ["1_month_from_now", atDay(30), true],
    ["1_month_from_now", new Date(Date.parse(atDay(30)) - 1).toISOString(), false],
  ];
  it.each(cases)("%s matches %s = %s", (preset, created, expected) => {
    expect(matchesCreationPreset(created, preset, NOW)).toBe(expected);
  });

  it("evaluates every cutoff from the local start of today", () => {
    expect(Date.parse(atDay(0))).toBe(localStartOfToday);
    expect(matchesCreationPreset(new Date(localStartDaysAgo(7)).toISOString(), "last_7_days", NOW)).toBe(true);
    expect(matchesCreationPreset(new Date(localStartDaysAgo(7) - 1).toISOString(), "last_7_days", NOW)).toBe(false);
    expect(matchesCreationPreset(new Date(localStartDaysAgo(30)).toISOString(), "last_30_days", NOW)).toBe(true);
    expect(matchesCreationPreset(new Date(localStartDaysAgo(30) - 1).toISOString(), "last_30_days", NOW)).toBe(false);
  });

  it("rejects an unparseable timestamp and labels every preset", () => {
    expect(matchesCreationPreset("not-a-date", "today", NOW)).toBe(false);
    for (const preset of PAGE_DATE_PRESETS) {
      expect(preset.label.length).toBeGreaterThan(0);
    }
  });

  it("matches and labels one explicit calendar day", () => {
    const pickedDay = new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate() - 3);
    const picked = `${pickedDay.getFullYear()}-${String(pickedDay.getMonth() + 1).padStart(2, "0")}-${String(
      pickedDay.getDate()
    ).padStart(2, "0")}`;
    expect(
      matchesCreationPreset(new Date(pickedDay.getTime() + 12 * 3_600_000).toISOString(), { custom: picked }, NOW)
    ).toBe(true);
    expect(
      matchesCreationPreset(new Date(pickedDay.getTime() + 36 * 3_600_000).toISOString(), { custom: picked }, NOW)
    ).toBe(false);
    expect(matchesCreationPreset(atDay(0, 12), { custom: "bogus" }, NOW)).toBe(false);
    expect(formatCustomDate("2026-10-20")).toContain("2026");
    expect(formatCustomDate("bogus")).toBe("bogus");
  });

  it("combines dimensions with AND", () => {
    const row = makePageRow({ created_at: atDay(0, 9), is_favorite: true, owned_by: "user-1" });
    expect(matchesPageFilters(row, EMPTY_PAGE_FILTERS, NOW)).toBe(true);
    expect(matchesPageFilters(row, { ...EMPTY_PAGE_FILTERS, favorites: true }, NOW)).toBe(true);
    expect(matchesPageFilters({ ...row, is_favorite: false }, { ...EMPTY_PAGE_FILTERS, favorites: true }, NOW)).toBe(
      false
    );
    expect(matchesPageFilters(row, { ...EMPTY_PAGE_FILTERS, favorites: true, creator: "user-2" }, NOW)).toBe(false);
    expect(matchesPageFilters(row, { ...EMPTY_PAGE_FILTERS, created: "last_30_days" }, NOW)).toBe(true);
    expect(matchesPageFilters(row, { ...EMPTY_PAGE_FILTERS, created: "1_week_ago" }, NOW)).toBe(false);
  });

  it("reports whether any dimension is active", () => {
    expect(hasPageFilters(EMPTY_PAGE_FILTERS)).toBe(false);
    expect(hasPageFilters({ ...EMPTY_PAGE_FILTERS, favorites: true })).toBe(true);
    expect(hasPageFilters({ ...EMPTY_PAGE_FILTERS, created: "today" })).toBe(true);
    expect(hasPageFilters({ ...EMPTY_PAGE_FILTERS, creator: "u1" })).toBe(true);
  });
});

describe("visiblePageRows", () => {
  const rows = [
    makePageRow({ id: "open-old", name: "Alpha", access: 0, updated_at: "2026-01-01T00:00:00Z" }),
    makePageRow({ id: "open-new", name: "Beta", access: 0, updated_at: "2026-02-01T00:00:00Z" }),
    makePageRow({ id: "private", name: "Gamma", access: 1, updated_at: "2026-03-01T00:00:00Z" }),
    makePageRow({ id: "archived", name: "Delta", access: 0, archived_at: "2026-04-01T00:00:00Z" }),
  ];

  it("applies tab, sort, search and filters in order", () => {
    expect(
      visiblePageRows(rows, "public", DEFAULT_PAGE_SORT, "", EMPTY_PAGE_FILTERS, NOW).map((row) => row.id)
    ).toEqual(["open-new", "open-old"]);
    expect(
      visiblePageRows(rows, "public", DEFAULT_PAGE_SORT, "alp", EMPTY_PAGE_FILTERS, NOW).map((row) => row.id)
    ).toEqual(["open-old"]);
    expect(
      visiblePageRows(rows, "private", DEFAULT_PAGE_SORT, "", EMPTY_PAGE_FILTERS, NOW).map((row) => row.id)
    ).toEqual(["private"]);
    expect(
      visiblePageRows(rows, "archived", DEFAULT_PAGE_SORT, "", EMPTY_PAGE_FILTERS, NOW).map((row) => row.id)
    ).toEqual(["archived"]);
  });
});

describe("chips and creator options", () => {
  it("labels active filters and removes only the requested one", () => {
    const filters = { favorites: true, created: "1_week_ago" as const, creator: "u1" };
    expect(pageFilterChips(filters, "Ada Byron")).toEqual([
      { id: "favorites", label: "Favorites", value: "Favorites" },
      { id: "created", label: "Created At", value: "1 week ago" },
      { id: "creator", label: "Created By", value: "Ada Byron" },
    ]);
    expect(pageFilterChips({ ...filters, creator: "missing" }, null)[2]?.value).toBe("Unknown member");
    expect(pageFilterChips({ ...EMPTY_PAGE_FILTERS, created: "bogus" as never }, null)[0]?.value).toBe("bogus");
    expect(pageFilterChips(EMPTY_PAGE_FILTERS, null)).toEqual([]);
    expect(pageFilterChips({ ...EMPTY_PAGE_FILTERS, created: { custom: "2026-10-20" } }, null)[0]?.value).toContain(
      "2026"
    );
    expect(removePageFilter(filters, "favorites")).toEqual({ ...filters, favorites: false });
    expect(removePageFilter(filters, "created")).toEqual({ ...filters, created: null });
    expect(removePageFilter(filters, "creator")).toEqual({ ...filters, creator: null });
  });

  it("renames the current user to You, searches and sorts creators", () => {
    const options = [
      { id: "u1", name: "Ada Byron", avatarUrl: null },
      { id: "u2", name: "Zed Shaw", avatarUrl: "https://x.test/z.png" },
    ];
    expect(selectPageCreators(options, "", "u1").map((option) => option.name)).toEqual(["You", "Zed Shaw"]);
    expect(selectPageCreators(options, "ada", "u2").map((option) => option.name)).toEqual(["Ada Byron"]);
    expect(selectPageCreators(options, "zzz", "u1")).toEqual([]);
  });

  it("resolves a creator filter to its visible name", () => {
    const options = [
      { id: "u1", name: "Ada Byron", avatarUrl: null },
      { id: "u2", name: "Zed Shaw", avatarUrl: null },
    ];
    expect(pageCreatorName(options, null)).toBeNull();
    expect(pageCreatorName(options, "u2")).toBe("Zed Shaw");
    expect(pageCreatorName(options, "u3")).toBeNull();
  });

  it("resolves the signed-in id only once the session read has data", () => {
    expect(pageSessionUserId(undefined)).toBeNull();
    expect(pageSessionUserId({ id: "u1" })).toBe("u1");
  });
});
