// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { PrefsRecord } from "./entities.js";
import { buildPrefsPatch, parseDisplayPrefs, selectDisplayPrefs } from "./displayPrefs.js";

function record(overrides: Partial<PrefsRecord> = {}): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
    ...overrides,
  };
}

describe("parseDisplayPrefs", () => {
  it("parses a fully populated record", () => {
    const prefs = parseDisplayPrefs(
      {
        layout: "kanban",
        group_by: "state",
        sub_group_by: "priority",
        order_by: "-created_at",
        show_empty_groups: true,
        sub_issue: true,
        calendar: { layout: "week", show_weekends: false },
      },
      { state: false, priority: true }
    );
    expect(prefs).toEqual({
      layout: "kanban",
      groupBy: "state",
      subGroupBy: "priority",
      orderBy: "-created_at",
      showEmptyGroups: true,
      showSubIssues: true,
      calendar: { mode: "week", showWeekends: false },
      properties: { state: false, priority: true },
    });
  });

  it("defaults an empty record to the list layout, hidden empties and shown sub-issues", () => {
    expect(parseDisplayPrefs({}, {})).toEqual({
      layout: "list",
      groupBy: null,
      subGroupBy: null,
      orderBy: "sort_order",
      showEmptyGroups: false,
      showSubIssues: true,
      calendar: { mode: "month", showWeekends: true },
      properties: {},
    });
  });

  it("falls back per field on unknown or mistyped values", () => {
    const prefs = parseDisplayPrefs(
      {
        layout: "board",
        group_by: 7,
        sub_group_by: ["state"],
        order_by: "",
        show_empty_groups: "yes",
        sub_issue: 1,
        calendar: "june",
      },
      { state: "no", priority: true }
    );
    expect(prefs.layout).toBe("list");
    expect(prefs.groupBy).toBeNull();
    expect(prefs.subGroupBy).toBeNull();
    expect(prefs.orderBy).toBe("sort_order");
    expect(prefs.showEmptyGroups).toBe(false);
    expect(prefs.showSubIssues).toBe(true);
    expect(prefs.calendar).toEqual({ mode: "month", showWeekends: true });
    expect(prefs.properties).toEqual({ priority: true });
  });

  it("keeps a mistyped order and null calendar read as defaults", () => {
    expect(parseDisplayPrefs({ order_by: 42 }, {}).orderBy).toBe("sort_order");
    expect(parseDisplayPrefs({ calendar: null }, {}).calendar).toEqual({ mode: "month", showWeekends: true });
    expect(parseDisplayPrefs({ calendar: { layout: "month" } }, {}).calendar).toEqual({
      mode: "month",
      showWeekends: true,
    });
  });

  it("reads a non-record properties value as empty", () => {
    expect(parseDisplayPrefs({}, null as unknown as Record<string, unknown>).properties).toEqual({});
  });

  it("selects the display keys off a preference record", () => {
    expect(selectDisplayPrefs(record()).layout).toBe("list");
    expect(selectDisplayPrefs(record({ display_filters: { layout: "calendar" } })).layout).toBe("calendar");
  });
});

describe("buildPrefsPatch", () => {
  it("sends complete key objects with the change merged in", () => {
    const patch = buildPrefsPatch(record(), { layout: "kanban" });
    expect(patch).toEqual({
      display_filters: {
        layout: "kanban",
        group_by: null,
        order_by: "sort_order",
        sub_group_by: null,
        show_empty_groups: false,
        sub_issue: true,
        calendar: { layout: "month", show_weekends: true },
      },
      display_properties: {},
    });
    expect(patch.filters).toBeUndefined();
    expect(patch.rich_filters).toBeUndefined();
  });

  it("writes the sub-issue switch explicitly on every patch", () => {
    const patch = buildPrefsPatch(record({ display_filters: { layout: "list", sub_issue: true } }), {
      showEmptyGroups: true,
    });
    expect(patch.display_filters?.["sub_issue"]).toBe(true);
    const flipped = buildPrefsPatch(record({ display_filters: { layout: "list" } }), { showSubIssues: true });
    expect(flipped.display_filters?.["sub_issue"]).toBe(true);
  });

  it("keeps unknown stored keys riding along inside each object", () => {
    const patch = buildPrefsPatch(
      record({
        display_filters: {
          layout: "list",
          future_key: "kept",
          calendar: { layout: "month", future_cal: 1 },
        },
        display_properties: { future_prop: "kept" },
      }),
      { orderBy: "-created_at" }
    );
    expect(patch.display_filters?.["future_key"]).toBe("kept");
    expect(patch.display_filters?.["order_by"]).toBe("-created_at");
    expect(patch.display_filters?.["calendar"]).toEqual({ layout: "month", future_cal: 1, show_weekends: true });
    expect(patch.display_properties?.["future_prop"]).toBe("kept");
  });

  it("merges a partial calendar update over the stored options", () => {
    const patch = buildPrefsPatch(
      record({ display_filters: { layout: "calendar", calendar: { layout: "month", show_weekends: false } } }),
      { calendar: { mode: "week", showWeekends: false } }
    );
    expect(patch.display_filters?.["calendar"]).toEqual({ layout: "week", show_weekends: false });
  });

  it("replaces a non-object stored calendar with a clean one", () => {
    const patch = buildPrefsPatch(record({ display_filters: { layout: "calendar", calendar: "june" } }), {
      layout: "calendar",
    });
    expect(patch.display_filters?.["calendar"]).toEqual({ layout: "month", show_weekends: true });
  });

  it("merges property visibility over the stored map", () => {
    const patch = buildPrefsPatch(record({ display_properties: { state: true } }), {
      properties: { state: false, priority: true },
    });
    expect(patch.display_properties).toEqual({ state: false, priority: true });
  });
});
