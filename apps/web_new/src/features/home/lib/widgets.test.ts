// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { HomePreferenceList } from "@pidash/api-client";

import { orderWidgetPrefs, visibleWidgetPrefs, widgetSectionKind } from "./widgets.js";

function pref(key: string, sort_order: number, is_enabled = true): HomePreferenceList[number] {
  return { key: key as "recents", is_enabled, config: {}, sort_order };
}

describe("orderWidgetPrefs", () => {
  it("sorts highest position first without mutating", () => {
    const prefs = [pref("recents", 99), pref("quick_links", 100)];
    expect(orderWidgetPrefs(prefs).map((entry) => entry.key)).toEqual(["quick_links", "recents"]);
    expect(prefs.map((entry) => entry.key)).toEqual(["recents", "quick_links"]);
  });

  it("keeps server order on ties", () => {
    const prefs = [pref("a", 5), pref("b", 5)];
    expect(orderWidgetPrefs(prefs).map((entry) => entry.key)).toEqual(["a", "b"]);
  });
});

describe("visibleWidgetPrefs", () => {
  it("keeps toggled-on widgets only", () => {
    const prefs = [pref("recents", 99, true), pref("quick_links", 100, false)];
    expect(visibleWidgetPrefs(prefs).map((entry) => entry.key)).toEqual(["recents"]);
  });
});

describe("widgetSectionKind", () => {
  it("resolves owned sections and parks the rest", () => {
    expect(widgetSectionKind("recents")).toBe("recents");
    expect(widgetSectionKind("quick_links")).toBe("quicklinks");
    expect(widgetSectionKind("my_stickies")).toBe("other");
    expect(widgetSectionKind("new_at_pi_dash")).toBe("other");
  });
});
