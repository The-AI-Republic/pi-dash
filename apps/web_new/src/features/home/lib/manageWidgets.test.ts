// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { HomePreferenceList } from "@pidash/api-client";

import { planWidgetReorder, surfacedWidgetPrefs, widgetDisplayName } from "./manageWidgets.js";

function pref(key: string, sort_order: number, is_enabled = true): HomePreferenceList[number] {
  return { key: key as "recents", is_enabled, config: {}, sort_order };
}

describe("surfacedWidgetPrefs", () => {
  it("keeps the rendered sections in stack order", () => {
    expect(surfacedWidgetPrefs([pref("recents", 99), pref("quick_links", 100)]).map((entry) => entry.key)).toEqual([
      "quick_links",
      "recents",
    ]);
  });

  it("drops the retired key and non-rendered keys", () => {
    expect(
      surfacedWidgetPrefs([
        pref("recents", 99),
        pref("my_stickies", 101),
        pref("new_at_pi_dash", 102),
        pref("quick_tutorial", 103),
      ]).map((entry) => entry.key)
    ).toEqual(["recents"]);
  });
});

describe("widgetDisplayName", () => {
  it("names the surfaced widgets", () => {
    expect(widgetDisplayName("quick_links")).toBe("Quicklinks");
    expect(widgetDisplayName("recents")).toBe("Recents");
  });

  it("falls back to the raw key", () => {
    expect(widgetDisplayName("my_stickies")).toBe("my_stickies");
  });
});

describe("planWidgetReorder", () => {
  const prefs = [pref("quick_links", 100), pref("recents", 99)];

  it("lands a top drop above every known sort", () => {
    expect(planWidgetReorder(prefs, "recents", 0)).toEqual({ key: "recents", sort_order: 101 });
  });

  it("lands a bottom drop below the row above it", () => {
    expect(planWidgetReorder(prefs, "quick_links", 1)).toEqual({ key: "quick_links", sort_order: 98 });
  });

  it("ignores non-rendered keys when planning", () => {
    const withRetired: HomePreferenceList = [...prefs, pref("my_stickies", 500)];
    expect(planWidgetReorder(withRetired, "quick_links", 1)).toEqual({ key: "quick_links", sort_order: 98 });
  });

  it("clamps drops past the ends", () => {
    expect(planWidgetReorder(prefs, "recents", -4)).toEqual({ key: "recents", sort_order: 101 });
    expect(planWidgetReorder(prefs, "quick_links", 9)).toEqual({ key: "quick_links", sort_order: 98 });
  });

  it("returns null for an unknown key", () => {
    expect(planWidgetReorder(prefs, "my_stickies", 0)).toBeNull();
  });

  it("returns null for a single row", () => {
    expect(planWidgetReorder([pref("recents", 99)], "recents", 0)).toBeNull();
  });

  it("returns null when the drop keeps the row's slot", () => {
    expect(planWidgetReorder(prefs, "quick_links", 0)).toBeNull();
    expect(planWidgetReorder(prefs, "recents", 1)).toBeNull();
  });
});
