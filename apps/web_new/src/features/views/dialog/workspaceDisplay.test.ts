// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { parseDisplayPrefs } from "../../issues/index.js";
import {
  WORKSPACE_VIEW_LAYOUT,
  WORKSPACE_VIEW_ORDER_BY,
  defaultWorkspaceDisplay,
  prefillWorkspaceDisplay,
  serializeWorkspaceDisplay,
} from "./workspaceDisplay.js";

describe("defaultWorkspaceDisplay", () => {
  it("opens on the spreadsheet layout ordered newest first", () => {
    expect(defaultWorkspaceDisplay()).toEqual({
      ...parseDisplayPrefs({}, {}),
      layout: "spreadsheet",
      orderBy: "-created_at",
    });
  });
});

describe("prefillWorkspaceDisplay", () => {
  it("keeps the stored settings and pins the layout", () => {
    const prefs = prefillWorkspaceDisplay(
      { layout: "list", group_by: "priority", order_by: "-updated_at" },
      { priority: true }
    );
    expect(prefs).toEqual({
      ...parseDisplayPrefs({ layout: "list", group_by: "priority", order_by: "-updated_at" }, { priority: true }),
      layout: "spreadsheet",
    });
    expect(prefs.groupBy).toBe("priority");
    expect(prefs.orderBy).toBe("-updated_at");
  });
});

describe("serializeWorkspaceDisplay", () => {
  it("writes every display key with the spreadsheet layout", () => {
    expect(serializeWorkspaceDisplay(defaultWorkspaceDisplay())).toEqual({
      displayFilters: {
        layout: "spreadsheet",
        group_by: null,
        sub_group_by: null,
        order_by: "-created_at",
        show_empty_groups: false,
        sub_issue: true,
        calendar: { layout: "month", show_weekends: false },
      },
      displayProperties: {},
    });
    expect(WORKSPACE_VIEW_LAYOUT).toBe("spreadsheet");
    expect(WORKSPACE_VIEW_ORDER_BY).toBe("-created_at");
  });

  it("pins a stored layout back to spreadsheet", () => {
    const prefs = { ...defaultWorkspaceDisplay(), layout: "kanban" as const };
    expect(serializeWorkspaceDisplay(prefs).displayFilters["layout"]).toBe("spreadsheet");
  });
});
