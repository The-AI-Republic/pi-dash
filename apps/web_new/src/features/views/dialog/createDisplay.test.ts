// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { parseDisplayPrefs } from "../../issues/index.js";
import {
  CREATE_BOARD_GROUP_BY,
  defaultCreateDisplay,
  normalizeCreateDisplay,
  serializeCreateDisplay,
} from "./createDisplay.js";

describe("defaultCreateDisplay", () => {
  it("opens on the List layout grouped by state", () => {
    expect(defaultCreateDisplay()).toEqual({ ...parseDisplayPrefs({}, {}), layout: "list", groupBy: "state" });
  });
});

describe("normalizeCreateDisplay", () => {
  it("groups an ungrouped Board by state", () => {
    const prefs = { ...defaultCreateDisplay(), layout: "kanban" as const, groupBy: null };
    expect(normalizeCreateDisplay(prefs).groupBy).toBe(CREATE_BOARD_GROUP_BY);
  });

  it("groups a blank-grouped Board by state", () => {
    const prefs = { ...defaultCreateDisplay(), layout: "kanban" as const, groupBy: "" };
    expect(normalizeCreateDisplay(prefs).groupBy).toBe(CREATE_BOARD_GROUP_BY);
  });

  it("keeps an explicit Board grouping", () => {
    const prefs = { ...defaultCreateDisplay(), layout: "kanban" as const, groupBy: "priority" };
    expect(normalizeCreateDisplay(prefs)).toBe(prefs);
  });

  it("leaves other layouts alone", () => {
    const prefs = { ...defaultCreateDisplay(), groupBy: null };
    expect(normalizeCreateDisplay(prefs)).toBe(prefs);
  });
});

describe("serializeCreateDisplay", () => {
  it("writes every display key with the layout always present", () => {
    expect(serializeCreateDisplay(defaultCreateDisplay())).toEqual({
      displayFilters: {
        layout: "list",
        group_by: "state",
        sub_group_by: null,
        order_by: "sort_order",
        show_empty_groups: false,
        sub_issue: true,
        calendar: { layout: "month", show_weekends: false },
      },
      displayProperties: {},
    });
  });

  it("carries an edited Board and its properties through", () => {
    const prefs = {
      ...defaultCreateDisplay(),
      layout: "kanban" as const,
      properties: { priority: true, assignee: false },
    };
    const payload = serializeCreateDisplay(prefs);
    expect(payload.displayFilters["layout"]).toBe("kanban");
    expect(payload.displayProperties).toEqual({ priority: true, assignee: false });
  });
});
