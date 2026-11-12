// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules list header model (MOD-004/005/006/007/010): search, the six
// ordering keys and their direction rule, the filter predicates and date
// tokens, member option shaping, chip groups and the no-match decision.

import { describe, expect, it } from "vitest";

import { makeModuleRow } from "../testing.js";
import {
  clearModuleDateRange,
  clearModuleFilterDimension,
  clearModuleFilters,
  compareModuleNaturalNames,
  DEFAULT_MODULE_SORT,
  defaultModuleCustomRange,
  moduleCustomWindow,
  emptyModuleFilters,
  hasModuleFilters,
  isModuleFilterDimensionActive,
  isModuleSortDirection,
  isModuleSortKey,
  matchesModuleMenuQuery,
  MODULE_DATE_PRESETS,
  MODULE_FILTER_CHIP_KEYS,
  MODULE_SORT_LABELS,
  MODULE_SORT_OPTIONS,
  MODULE_STATUS_OPTIONS,
  moduleDatePresetDay,
  moduleDateRange,
  moduleDateTokenKey,
  moduleListContentState,
  moduleMatchesDateDimension,
  moduleMatchesFilters,
  moduleMatchesSearch,
  moduleMemberName,
  moduleProgressShare,
  moduleSortHasDirection,
  moduleStatusColor,
  moduleStatusLabel,
  orderModules,
  removeModuleFilterValue,
  searchModules,
  selectModuleChipGroups,
  selectModuleMemberOptions,
  selectModuleSort,
  toggleModuleDateToken,
  toggleModuleFilterValue,
  visibleModules,
  type ModuleListFilters,
  type ModuleSort,
} from "./moduleListControls.js";

const ME = "423e4567-e89b-12d3-a456-426614174003";
const OTHER = "623e4567-e89b-12d3-a456-426614174005";

function member(id: string, display: string, role = 15, overrides: Record<string, unknown> = {}) {
  return {
    id: `membership-${id}`,
    role,
    member: {
      id,
      first_name: display,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: display,
      is_bot: false,
      ...overrides,
    },
  };
}

/** Fixed clock: 2026-10-20 local noon keeps date offsets deterministic. */
const NOW = new Date(2026, 9, 20, 12, 0, 0);

describe("search (MOD-004)", () => {
  const rows = [makeModuleRow({ name: "Search Alpha" }), makeModuleRow({ name: "search Bravo" })];

  it("matches a case-insensitive name substring", () => {
    expect(moduleMatchesSearch(rows[0]!, "alpha")).toBe(true);
    expect(moduleMatchesSearch(rows[1]!, "SEARCH")).toBe(true);
    expect(moduleMatchesSearch(rows[0]!, "zzz")).toBe(false);
  });

  it("keeps every row for a blank or whitespace query", () => {
    expect(searchModules(rows, "")).toEqual(rows);
    expect(searchModules(rows, "   ")).toEqual(rows);
    expect(searchModules([], "alpha")).toEqual([]);
    expect(moduleMatchesSearch(rows[0]!, "")).toBe(true);
  });

  it("narrows by trimmed substring", () => {
    expect(searchModules(rows, " bravo ").map((row) => row.name)).toEqual(["search Bravo"]);
  });
});

describe("ordering keys (MOD-005)", () => {
  it("offers the six pinned labels in spec order", () => {
    expect(MODULE_SORT_OPTIONS.map((option) => option.label)).toEqual([
      "Name",
      "Progress",
      "Number of work items",
      "Due date",
      "Created date",
      "Manual",
    ]);
    expect(MODULE_SORT_LABELS.manual).toBe("Manual");
  });

  it("defaults to Name ascending and only Manual lacks a direction", () => {
    expect(DEFAULT_MODULE_SORT).toEqual({ key: "name", direction: "ascending" });
    expect(moduleSortHasDirection("manual")).toBe(false);
    expect(moduleSortHasDirection("name")).toBe(true);
  });

  it("validates keys, directions and selects a key or direction", () => {
    expect(isModuleSortKey("due-date")).toBe(true);
    expect(isModuleSortKey("bogus")).toBe(false);
    expect(isModuleSortDirection("descending")).toBe(true);
    expect(isModuleSortDirection("sideways")).toBe(false);
    expect(selectModuleSort({ key: "name", direction: "ascending" }, { key: "progress" })).toEqual({
      key: "progress",
      direction: "ascending",
    });
    expect(selectModuleSort({ key: "name", direction: "descending" }, { direction: "ascending" })).toEqual({
      key: "name",
      direction: "ascending",
    });
    expect(selectModuleSort({ key: "name", direction: "descending" }, { key: "manual" })).toEqual({
      key: "manual",
      direction: "ascending",
    });
  });

  it("compares names naturally and case-insensitively", () => {
    expect(compareModuleNaturalNames("Module 2", "Module 10")).toBeLessThan(0);
    expect(compareModuleNaturalNames("module 2", "MODULE 2")).toBe(0);
    expect(compareModuleNaturalNames("Alpha", "Beta")).toBeLessThan(0);
    expect(compareModuleNaturalNames("Beta", "Alpha")).toBeGreaterThan(0);
    expect(compareModuleNaturalNames("Module 2a", "Module 2b")).toBeLessThan(0);
    expect(compareModuleNaturalNames("Module 02", "Module 2")).toBeGreaterThan(0);
    expect(compareModuleNaturalNames("Module", "Module Alpha")).toBeLessThan(0);
    // The same comparisons read backwards flip the sign.
    expect(compareModuleNaturalNames("Module 10", "Module 2")).toBeGreaterThan(0);
    expect(compareModuleNaturalNames("Module 2", "Module 02")).toBeLessThan(0);
    expect(compareModuleNaturalNames("Module Alpha", "Module")).toBeGreaterThan(0);
    expect(compareModuleNaturalNames("Module Alpha Two", "Module Alpha")).toBeGreaterThan(0);
    // Neither side has a chunk at all (empty names).
    expect(compareModuleNaturalNames("", "")).toBe(0);
    // One side carries a chunk run the other lacks after an identical prefix.
    expect(compareModuleNaturalNames("Module1", "Module")).toBeGreaterThan(0);
    expect(compareModuleNaturalNames("Module", "Module1")).toBeLessThan(0);
  });

  it("reads progress as (completed + cancelled) / total, zero without work items", () => {
    expect(moduleProgressShare({ completed_issues: 2, cancelled_issues: 1, total_issues: 4 })).toBe(0.75);
    expect(moduleProgressShare({ completed_issues: 0, cancelled_issues: 0, total_issues: 0 })).toBe(0);
    expect(moduleProgressShare({ completed_issues: 1, cancelled_issues: 0, total_issues: 2 })).toBe(0.5);
  });

  it("keeps collection order for equal progress and equal unparseable dates", () => {
    const first = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174050",
      name: "Equal First",
      total_issues: 4,
      completed_issues: 2,
      cancelled_issues: 0,
      created_at: "same-bad-date",
    });
    const second = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174051",
      name: "Equal Second",
      total_issues: 2,
      completed_issues: 1,
      cancelled_issues: 0,
      created_at: "same-bad-date",
    });
    expect(orderModules([first, second], { key: "progress", direction: "ascending" }).map((row) => row.name)).toEqual([
      "Equal First",
      "Equal Second",
    ]);
    expect(
      orderModules([second, first], { key: "created-date", direction: "descending" }).map((row) => row.name)
    ).toEqual(["Equal Second", "Equal First"]);
  });

  it("orders by each key with stable ties and a direction flip", () => {
    const charlie = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174010",
      name: "Order Charlie",
      created_at: "2026-01-01T00:00:00Z",
      target_date: "2026-12-10",
      total_issues: 2,
      completed_issues: 0,
      cancelled_issues: 0,
      sort_order: 3,
    });
    const alpha = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174011",
      name: "Order Alpha",
      created_at: "2026-01-02T00:00:00Z",
      target_date: "2026-12-20",
      total_issues: 1,
      completed_issues: 1,
      sort_order: 1,
    });
    const bravo = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174012",
      name: "Order Bravo",
      created_at: "2026-01-03T00:00:00Z",
      target_date: "2026-12-15",
      total_issues: 3,
      completed_issues: 1,
      sort_order: 2,
    });
    const rows = [charlie, alpha, bravo];
    const names = (sort: ModuleSort) => orderModules(rows, sort).map((row) => row.name);
    expect(names({ key: "name", direction: "ascending" })).toEqual(["Order Alpha", "Order Bravo", "Order Charlie"]);
    expect(names({ key: "name", direction: "descending" })).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
    expect(names({ key: "due-date", direction: "ascending" })).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
    expect(names({ key: "created-date", direction: "ascending" })).toEqual([
      "Order Charlie",
      "Order Alpha",
      "Order Bravo",
    ]);
    expect(names({ key: "work-items", direction: "ascending" })).toEqual([
      "Order Alpha",
      "Order Charlie",
      "Order Bravo",
    ]);
    expect(names({ key: "progress", direction: "ascending" })).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
    expect(names({ key: "manual", direction: "ascending" })).toEqual(["Order Alpha", "Order Bravo", "Order Charlie"]);
  });

  it("keeps undated modules last in both due-date directions", () => {
    const undatedA = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174020",
      name: "Undated A",
      target_date: null,
    });
    const undatedB = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174021",
      name: "Undated B",
      target_date: null,
    });
    const early = makeModuleRow({
      id: "323e4567-e89b-12d3-a456-426614174022",
      name: "Early",
      target_date: "2026-01-01",
    });
    const late = makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174023", name: "Late", target_date: "2026-06-01" });
    const rows = [undatedA, early, undatedB, late];
    expect(orderModules(rows, { key: "due-date", direction: "ascending" }).map((row) => row.name)).toEqual([
      "Early",
      "Late",
      "Undated A",
      "Undated B",
    ]);
    expect(orderModules(rows, { key: "due-date", direction: "descending" }).map((row) => row.name)).toEqual([
      "Late",
      "Early",
      "Undated A",
      "Undated B",
    ]);
    // Either side of the pair can be the undated one.
    expect(orderModules([early, undatedA], { key: "due-date", direction: "ascending" }).map((row) => row.name)).toEqual(
      ["Early", "Undated A"]
    );
    expect(orderModules([undatedA, early], { key: "due-date", direction: "ascending" }).map((row) => row.name)).toEqual(
      ["Early", "Undated A"]
    );
  });

  it("falls back to string comparison for unparseable creation instants", () => {
    const a = makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174030", name: "A", created_at: "bad-a" });
    const b = makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174031", name: "B", created_at: "bad-b" });
    expect(orderModules([b, a], { key: "created-date", direction: "ascending" }).map((row) => row.name)).toEqual([
      "A",
      "B",
    ]);
    expect(orderModules([a, b], { key: "created-date", direction: "descending" }).map((row) => row.name)).toEqual([
      "B",
      "A",
    ]);
    // Equal unparseable instants keep the collection order.
    const equal = makeModuleRow({ created_at: "not-a-date" });
    expect(orderModules([b, equal], { key: "created-date", direction: "ascending" }).map((row) => row.name)).toEqual([
      "B",
      "Delivery",
    ]);
  });
});

describe("filter dimensions (MOD-006)", () => {
  it("lists the six statuses with labels and colors", () => {
    expect(MODULE_STATUS_OPTIONS.map((option) => option.label)).toEqual([
      "Backlog",
      "Planned",
      "In Progress",
      "Paused",
      "Completed",
      "Cancelled",
    ]);
    expect(moduleStatusLabel("in-progress")).toBe("In Progress");
    expect(moduleStatusColor("completed")).toBe("#16a34a");
    expect(moduleStatusColor("cancelled")).toBe("#ef4444");
    // A status outside the six still renders text and a neutral dot.
    expect(moduleStatusLabel("mystery" as never)).toBe("mystery");
    expect(moduleStatusColor("mystery" as never)).toBe("#a3a3a2");
  });

  it("toggles favorites and multi values without touching siblings", () => {
    let filters = emptyModuleFilters();
    expect(hasModuleFilters(filters)).toBe(false);
    filters = toggleModuleFilterValue(filters, "favorites");
    expect(filters.favorites).toBe(true);
    expect(isModuleFilterDimensionActive(filters, "favorites")).toBe(true);
    expect(isModuleFilterDimensionActive(filters, "status")).toBe(false);
    expect(isModuleFilterDimensionActive(toggleModuleFilterValue(filters, "lead", "x"), "lead")).toBe(true);
    filters = toggleModuleFilterValue(filters, "favorites");
    expect(filters.favorites).toBe(false);
    filters = toggleModuleFilterValue(filters, "status", "completed");
    filters = toggleModuleFilterValue(filters, "status", "planned");
    expect(filters.status).toEqual(["completed", "planned"]);
    filters = toggleModuleFilterValue(filters, "status", "completed");
    expect(filters.status).toEqual(["planned"]);
    expect(toggleModuleFilterValue(filters, "lead", "x").lead).toEqual(["x"]);
    expect(toggleModuleFilterValue(filters, "members", "y").members).toEqual(["y"]);
    const others = toggleModuleFilterValue(toggleModuleFilterValue(filters, "members", "y"), "members", "z");
    expect(others.members).toEqual(["y", "z"]);
    expect(toggleModuleFilterValue(toggleModuleFilterValue(filters, "lead", "x"), "lead", "x").lead).toEqual([]);
    expect(toggleModuleFilterValue(toggleModuleFilterValue(filters, "members", "y"), "members", "y").members).toEqual(
      []
    );
  });

  it("keeps a passed dimension value untouched where there is no handler", () => {
    const filters = emptyModuleFilters();
    expect(toggleModuleFilterValue(filters, "start-date")).toBe(filters);
  });

  it("narrows with favorites, status, lead and member ORs", () => {
    const row = makeModuleRow({
      status: "completed",
      lead_id: ME,
      member_ids: [OTHER],
      is_favorite: true,
      start_date: null,
      target_date: null,
    });
    expect(moduleMatchesFilters(row, emptyModuleFilters(), NOW)).toBe(true);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), favorites: true }, NOW)).toBe(true);
    expect(
      moduleMatchesFilters(makeModuleRow({ is_favorite: false }), { ...emptyModuleFilters(), favorites: true }, NOW)
    ).toBe(false);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), status: ["completed", "planned"] }, NOW)).toBe(true);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), status: ["planned"] }, NOW)).toBe(false);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), lead: [ME] }, NOW)).toBe(true);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), lead: [OTHER] }, NOW)).toBe(false);
    expect(moduleMatchesFilters(makeModuleRow({ lead_id: null }), { ...emptyModuleFilters(), lead: [ME] }, NOW)).toBe(
      false
    );
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), members: [OTHER] }, NOW)).toBe(true);
    expect(moduleMatchesFilters(row, { ...emptyModuleFilters(), members: [ME] }, NOW)).toBe(false);
    expect(
      moduleMatchesFilters(
        row,
        {
          ...emptyModuleFilters(),
          "due-date": [{ kind: "range", value: { start: "2026-10-01", end: "2026-10-19" } }],
        },
        NOW
      )
    ).toBe(false);
  });

  it("treats a relative choice as a lower bound and a range as inclusive", () => {
    const row = makeModuleRow({ start_date: "2026-10-30", target_date: "2026-11-10" });
    const week = toggleModuleDateToken(emptyModuleFilters(), "start-date", { kind: "preset", value: "week-1" });
    expect(moduleMatchesDateDimension(row, "start-date", week["start-date"], NOW)).toBe(true);
    const month = toggleModuleDateToken(emptyModuleFilters(), "start-date", { kind: "preset", value: "month-1" });
    expect(moduleMatchesDateDimension(row, "start-date", month["start-date"], NOW)).toBe(false);
    const range = toggleModuleDateToken(emptyModuleFilters(), "due-date", {
      kind: "range",
      value: { start: "2026-10-20", end: "2026-11-20" },
    });
    expect(moduleMatchesDateDimension(row, "due-date", range["due-date"], NOW)).toBe(true);
    const outside = toggleModuleDateToken(emptyModuleFilters(), "due-date", {
      kind: "range",
      value: { start: "2026-11-11", end: "2026-12-01" },
    });
    expect(moduleMatchesDateDimension(row, "due-date", outside["due-date"], NOW)).toBe(false);
    expect(moduleMatchesDateDimension(makeModuleRow({ start_date: null }), "start-date", week["start-date"], NOW)).toBe(
      false
    );
    expect(moduleMatchesDateDimension(row, "start-date", [], NOW)).toBe(true);
  });

  it("ANDs tokens inside a date dimension and groups across dimensions", () => {
    const row = makeModuleRow({ start_date: "2026-10-30", target_date: "2026-10-30", status: "planned" });
    let filters = toggleModuleDateToken(emptyModuleFilters(), "start-date", { kind: "preset", value: "week-1" });
    filters = toggleModuleDateToken(filters, "start-date", {
      kind: "range",
      value: { start: "2026-10-25", end: "2026-11-05" },
    });
    expect(moduleMatchesFilters(row, filters, NOW)).toBe(true);
    filters = toggleModuleDateToken(filters, "start-date", { kind: "preset", value: "month-2" });
    // The 60-day lower bound lies past the range's end, so the intersection is empty.
    expect(moduleMatchesFilters(row, filters, NOW)).toBe(false);
    expect(moduleMatchesFilters({ ...row, status: "backlog" }, { ...filters, status: ["planned"] }, NOW)).toBe(false);
  });

  it("combines filters with the name search", () => {
    const rows = [
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174040", name: "Alpha", status: "completed" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174041", name: "Bravo", status: "completed" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174042", name: "Alpha two", status: "planned" }),
    ];
    const filters = toggleModuleFilterValue(emptyModuleFilters(), "status", "completed");
    expect(visibleModules(rows, "alpha", filters, NOW).map((row) => row.name)).toEqual(["Alpha"]);
  });

  it("computes the custom default window and preset days", () => {
    expect(defaultModuleCustomRange(NOW)).toEqual({ start: "2026-10-20", end: "2026-11-20" });
    expect(defaultModuleCustomRange(new Date(2026, 0, 31, 12))).toEqual({ start: "2026-01-31", end: "2026-03-03" });
    expect(moduleCustomWindow(null, NOW)).toEqual({ start: "2026-10-20", end: "2026-11-20" });
    expect(moduleCustomWindow({ start: "2026-10-10", end: null }, NOW)).toEqual({
      start: "2026-10-10",
      end: "2026-11-20",
    });
    expect(moduleCustomWindow({ start: "2026-10-21", end: "2026-10-25" }, NOW)).toEqual({
      start: "2026-10-21",
      end: "2026-10-25",
    });
    expect(moduleDatePresetDay("week-1", NOW)).toBe("2026-10-27");
    expect(moduleDatePresetDay("month-2", NOW)).toBe("2026-12-19");
    expect(MODULE_DATE_PRESETS.map((preset) => preset.label)).toEqual([
      "1 week from now",
      "2 weeks from now",
      "1 month from now",
      "2 months from now",
    ]);
  });

  it("toggles tokens, reads and clears ranges, and keeps relative ones", () => {
    const range = { kind: "range" as const, value: { start: "2026-10-20", end: "2026-11-20" } };
    let filters = toggleModuleDateToken(emptyModuleFilters(), "due-date", range);
    expect(moduleDateRange(filters, "due-date")).toEqual(range.value);
    expect(moduleDateRange(filters, "start-date")).toBeNull();
    filters = toggleModuleDateToken(filters, "due-date", { kind: "preset", value: "week-1" });
    expect(filters["due-date"]).toHaveLength(2);
    filters = clearModuleDateRange(filters, "due-date");
    expect(filters["due-date"]).toEqual([{ kind: "preset", value: "week-1" }]);
    filters = toggleModuleDateToken(filters, "due-date", { kind: "preset", value: "week-1" });
    expect(filters["due-date"]).toEqual([]);
    // A different range is another token; the identical one cancels it out again.
    const other = { kind: "range" as const, value: { start: "2026-11-01", end: "2026-12-01" } };
    filters = toggleModuleDateToken(toggleModuleDateToken(filters, "due-date", range), "due-date", other);
    expect(filters["due-date"]).toEqual([range, other]);
    filters = toggleModuleDateToken(filters, "due-date", { ...range, value: { ...range.value } });
    expect(filters["due-date"]).toEqual([other]);
    // A token of another kind never matches, even with the same text.
    expect(moduleDateTokenKey(range)).toBe("2026-10-20..2026-11-20");
    expect(moduleDateTokenKey({ kind: "preset", value: "week-1" })).toBe("week-1");
  });

  it("removes one value or a whole dimension, and clears everything", () => {
    let filters: ModuleListFilters = {
      favorites: true,
      status: ["completed", "planned"],
      lead: [ME],
      members: [OTHER],
      "start-date": [{ kind: "preset", value: "week-1" }],
      "due-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
    };
    expect(removeModuleFilterValue(filters, "favorites", "Favorite").favorites).toBe(false);
    expect(removeModuleFilterValue(filters, "favorites", "other")).toBe(filters);
    expect(removeModuleFilterValue(filters, "status", "planned").status).toEqual(["completed"]);
    expect(removeModuleFilterValue(filters, "lead", ME).lead).toEqual([]);
    expect(removeModuleFilterValue(filters, "members", OTHER).members).toEqual([]);
    expect(removeModuleFilterValue(filters, "start-date", "week-1")["start-date"]).toEqual([]);
    expect(removeModuleFilterValue(filters, "due-date", "2026-10-20..2026-11-20")["due-date"]).toEqual([]);
    expect(clearModuleFilterDimension(filters, "status").status).toEqual([]);
    expect(clearModuleFilterDimension(filters, "favorites").favorites).toBe(false);
    expect(hasModuleFilters(clearModuleFilters())).toBe(false);
    filters = clearModuleFilters();
    expect(hasModuleFilters(filters)).toBe(false);
  });
});

describe("member options (MOD-006)", () => {
  const directory = [
    member(OTHER, "Zed Member"),
    member("723e4567-e89b-12d3-a456-426614174006", "Amy Bot", 15, { is_bot: true }),
    member(ME, "Rich Owner"),
    member("823e4567-e89b-12d3-a456-426614174007", "Guest Person", 5),
    member("923e4567-e89b-12d3-a456-426614174008", "Pat Selected"),
  ];

  it("puts You first, then selected members, and excludes guests", () => {
    const options = selectModuleMemberOptions({
      members: directory,
      meId: ME,
      selectedIds: ["923e4567-e89b-12d3-a456-426614174008"],
    });
    expect(options.map((option) => option.label)).toEqual(["You", "Pat Selected", "Zed Member"]);
    expect(options[0]?.searchText).toBe("Rich Owner");
  });

  it("lifts a selected member above unselected ones in both directions", () => {
    const options = selectModuleMemberOptions({
      members: [member(OTHER, "Zed Member"), member("923e4567-e89b-12d3-a456-426614174008", "Pat Selected")],
      meId: null,
      selectedIds: ["923e4567-e89b-12d3-a456-426614174008"],
    });
    expect(options.map((option) => option.label)).toEqual(["Pat Selected", "Zed Member"]);
    const reversed = selectModuleMemberOptions({
      members: [member("923e4567-e89b-12d3-a456-426614174008", "Pat Selected"), member(OTHER, "Zed Member")],
      meId: null,
      selectedIds: ["923e4567-e89b-12d3-a456-426614174008"],
    });
    expect(reversed.map((option) => option.label)).toEqual(["Pat Selected", "Zed Member"]);
  });

  it("returns nothing without a directory, and handles a null viewer", () => {
    expect(selectModuleMemberOptions({ members: undefined, meId: null, selectedIds: [] })).toEqual([]);
    expect(
      selectModuleMemberOptions({ members: directory, meId: null, selectedIds: [], excludeGuests: false })
    ).toHaveLength(4);
  });

  it("names a member, with You and a fallback", () => {
    expect(moduleMemberName({ members: directory, meId: ME }, ME)).toBe("You");
    expect(moduleMemberName({ members: directory, meId: null }, OTHER)).toBe("Zed Member");
    expect(moduleMemberName({ members: directory, meId: null }, "missing")).toBe("Unknown member");
  });

  it("narrows menu options case-insensitively on label and display text", () => {
    expect(matchesModuleMenuQuery("Cancelled", "", "cancel")).toBe(true);
    expect(matchesModuleMenuQuery("You", "Rich Owner", "rich")).toBe(true);
    expect(matchesModuleMenuQuery("Backlog", "", "cancel")).toBe(false);
    expect(matchesModuleMenuQuery("Backlog", "", " ")).toBe(true);
  });
});

describe("chip groups (MOD-007)", () => {
  const directory = [member(ME, "Rich Owner"), member(OTHER, "Zed Member")];
  const base = {
    members: directory,
    meId: ME,
    formatDateRange: (start: string, end: string) => `${start}..${end}`,
  };

  it("keeps the spec order and stable keys", () => {
    const filters: ModuleListFilters = {
      favorites: true,
      status: ["completed", "planned"],
      lead: [ME],
      members: [OTHER],
      "start-date": [{ kind: "preset", value: "week-1" }],
      "due-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
    };
    const groups = selectModuleChipGroups(filters, base);
    expect(groups.map((group) => group.key)).toEqual([
      "status",
      "lead",
      "members",
      "start date",
      "target date",
      "Modules",
    ]);
    expect(groups[0]?.values.map((value) => value.label)).toEqual(["Completed", "Planned"]);
    expect(groups[0]?.values[0]?.color).toBe("#16a34a");
    expect(groups[1]?.values[0]?.label).toBe("You");
    expect(groups[2]?.values[0]?.label).toBe("Zed Member");
    expect(groups[3]?.values.map((value) => value.label)).toEqual(["1 week from now"]);
    expect(groups[4]?.values.map((value) => value.label)).toEqual(["2026-10-20..2026-11-20"]);
    expect(groups[5]?.values).toEqual([{ value: "Favorite", label: "Favorite" }]);
    expect(MODULE_FILTER_CHIP_KEYS.favorites).toBe("Modules");
  });

  it("hides favorites and status for the archived variant", () => {
    const filters: ModuleListFilters = { ...emptyModuleFilters(), favorites: true, status: ["completed"], lead: [ME] };
    expect(
      selectModuleChipGroups(filters, { ...base, showFavorites: false, showStatus: false }).map((g) => g.key)
    ).toEqual(["lead"]);
  });

  it("is empty with no filters and names an unknown member", () => {
    expect(selectModuleChipGroups(emptyModuleFilters(), base)).toEqual([]);
    expect(selectModuleChipGroups({ ...emptyModuleFilters(), lead: ["missing"] }, base)[0]?.values[0]?.label).toBe(
      "Unknown member"
    );
  });
});

describe("no-match (MOD-010)", () => {
  it("shows only when modules exist but none are visible", () => {
    expect(moduleListContentState(2, 0)).toBe("no-match");
    expect(moduleListContentState(0, 0)).toBe("rows");
    expect(moduleListContentState(2, 2)).toBe("rows");
  });

  it("restores the untouched set when narrowing is cleared", () => {
    const rows = [makeModuleRow({ name: "Alpha" }), makeModuleRow({ name: "Bravo" })];
    expect(visibleModules(rows, "zzz", emptyModuleFilters(), NOW)).toEqual([]);
    expect(visibleModules(rows, "", clearModuleFilters(), NOW)).toEqual(rows);
  });
});
