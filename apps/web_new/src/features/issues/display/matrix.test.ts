// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { LayoutKind } from "../layouts/kinds.js";
import {
  DISPLAY_SECTION_ORDER,
  DISPLAY_SECTION_TITLES,
  DISPLAY_SWITCH_LABELS,
  EPIC_SUB_ISSUE_COUNT_LABEL,
  GROUP_DIMENSION_LABELS,
  GROUP_NONE_LABEL,
  archivedDisplayMatrix,
  groupDimensionLabel,
  isPropertyShown,
  layoutDisplayMatrix,
  ORDER_OPTION_LABELS,
  PROPERTY_PILL_LABELS,
  propertyPillLabel,
  visibleGroupDimensions,
  visiblePropertyKeys,
} from "./matrix.js";

const LAYOUTS: readonly LayoutKind[] = ["list", "kanban", "calendar", "spreadsheet", "gantt_chart"];

describe("layoutDisplayMatrix", () => {
  it("lists the five sections in fixed order with a title each", () => {
    expect(DISPLAY_SECTION_ORDER).toEqual(["properties", "group", "subGroup", "order", "options"]);
    for (const section of DISPLAY_SECTION_ORDER) {
      expect(DISPLAY_SECTION_TITLES[section].length).toBeGreaterThan(0);
    }
    expect(DISPLAY_SECTION_TITLES.properties).toBe("Display Properties");
    expect(DISPLAY_SECTION_TITLES.group).toBe("Group by");
    expect(DISPLAY_SECTION_TITLES.subGroup).toBe("Sub-group by");
    expect(DISPLAY_SECTION_TITLES.order).toBe("Order by");
  });

  it("offers the full panel on list with a clearing choice", () => {
    const matrix = layoutDisplayMatrix("list");
    expect(matrix.properties).toHaveLength(13);
    expect(matrix.groupBy).toEqual([
      "state",
      "state_detail.group",
      "priority",
      "cycle",
      "module",
      "labels",
      "assignees",
      "created_by",
      "team_project",
      null,
    ]);
    expect(matrix.subGroupBy).toBeNull();
    expect(matrix.orderBy).toEqual([
      "sort_order",
      "-created_at",
      "-updated_at",
      "start_date",
      "-priority",
      "target_date",
    ]);
    expect(matrix.switches).toEqual(["sub_issue", "show_empty_groups"]);
  });

  it("adds the sub-group axis on kanban without a group clearing choice", () => {
    const matrix = layoutDisplayMatrix("kanban");
    expect(matrix.properties).toHaveLength(13);
    expect(matrix.groupBy).not.toContain(null);
    expect(matrix.subGroupBy).toContain(null);
    expect(matrix.orderBy).toHaveLength(6);
    expect(matrix.switches).toEqual(["sub_issue", "show_empty_groups"]);
  });

  it("keeps calendar minimal: identifier pill plus the sub-issue switch", () => {
    expect(layoutDisplayMatrix("calendar")).toEqual({
      properties: ["key"],
      groupBy: null,
      subGroupBy: null,
      orderBy: null,
      switches: ["sub_issue"],
    });
  });

  it("keeps spreadsheet pills with ordering and the sub-issue switch", () => {
    const matrix = layoutDisplayMatrix("spreadsheet");
    expect(matrix.properties).toHaveLength(13);
    expect(matrix.groupBy).toBeNull();
    expect(matrix.subGroupBy).toBeNull();
    expect(matrix.orderBy).toEqual(["sort_order", "-created_at", "-updated_at", "start_date", "-priority"]);
    expect(matrix.switches).toEqual(["sub_issue"]);
  });

  it("keeps gantt minimal with ordering and the sub-issue switch", () => {
    expect(layoutDisplayMatrix("gantt_chart")).toEqual({
      properties: ["key"],
      groupBy: null,
      subGroupBy: null,
      orderBy: ["sort_order", "-created_at", "-updated_at", "start_date", "-priority"],
      switches: ["sub_issue"],
    });
  });

  it("limits the archived panel to the spec §2.3 grouping and ordering", () => {
    const matrix = archivedDisplayMatrix();
    expect(matrix.groupBy).toEqual(["state", "cycle", "module", "priority", "labels", "assignees", "created_by", null]);
    expect(matrix.orderBy).toEqual(["sort_order", "-created_at", "-updated_at", "start_date", "-priority"]);
    expect(matrix.subGroupBy).toBeNull();
    expect(matrix.switches).toEqual(["show_empty_groups"]);
  });

  it("labels every matrix key", () => {
    for (const layout of LAYOUTS) {
      const matrix = layoutDisplayMatrix(layout);
      for (const key of matrix.properties) expect(PROPERTY_PILL_LABELS[key]).toBeDefined();
      for (const key of matrix.orderBy ?? []) expect(ORDER_OPTION_LABELS[key]).toBeDefined();
      for (const key of [...(matrix.groupBy ?? []), ...(matrix.subGroupBy ?? [])]) {
        if (key !== null) expect(GROUP_DIMENSION_LABELS[key]).toBeDefined();
      }
      for (const key of matrix.switches) expect(DISPLAY_SWITCH_LABELS[key]).toBeDefined();
    }
    expect(ORDER_OPTION_LABELS["-created_at"]).toBe("Last created");
    expect(GROUP_DIMENSION_LABELS["state"]).toBe("States");
    expect(GROUP_DIMENSION_LABELS["state_detail.group"]).toBe("State groups");
  });
});

describe("visibleGroupDimensions", () => {
  const flags = { hideCycle: false, hideModule: false };

  it("hides the other axis selection on kanban but never the clearing choice", () => {
    const dimensions = ["state", "priority", null] as const;
    expect(visibleGroupDimensions(dimensions, "state", flags)).toEqual(["priority", null]);
    expect(visibleGroupDimensions(dimensions, null, flags)).toEqual(["state", "priority", null]);
  });

  it("suppresses cycle and module while their features are off", () => {
    const dimensions = ["state", "cycle", "module", null] as const;
    expect(visibleGroupDimensions(dimensions, null, { hideCycle: true, hideModule: false })).toEqual([
      "state",
      "module",
      null,
    ]);
    expect(visibleGroupDimensions(dimensions, null, { hideCycle: true, hideModule: true })).toEqual(["state", null]);
  });
});

describe("visiblePropertyKeys", () => {
  it("suppresses cycle and module pills while their features are off", () => {
    expect(visiblePropertyKeys(["key", "cycle", "modules"], { hideCycle: true, hideModule: true })).toEqual(["key"]);
    expect(visiblePropertyKeys(["key", "cycle", "modules"], { hideCycle: false, hideModule: false })).toEqual([
      "key",
      "cycle",
      "modules",
    ]);
  });
});

describe("isPropertyShown", () => {
  it("reads absent keys as shown and only explicit false as hidden", () => {
    expect(isPropertyShown({}, "state")).toBe(true);
    expect(isPropertyShown({ state: true }, "state")).toBe(true);
    expect(isPropertyShown({ state: false }, "state")).toBe(false);
  });
});

describe("labels", () => {
  it("relabels the sub-issue count on epic surfaces", () => {
    expect(propertyPillLabel("sub_issue_count", false)).toBe("Sub-work item count");
    expect(propertyPillLabel("sub_issue_count", true)).toBe(EPIC_SUB_ISSUE_COUNT_LABEL);
    expect(propertyPillLabel("state", true)).toBe("State");
  });

  it("falls back to the raw key for unknown properties", () => {
    expect(propertyPillLabel("future_prop", false)).toBe("future_prop");
  });

  it("reads the null dimension as the ungrouped choice", () => {
    expect(groupDimensionLabel(null)).toBe(GROUP_NONE_LABEL);
    expect(groupDimensionLabel("state")).toBe("States");
    expect(groupDimensionLabel("future_dim")).toBe("future_dim");
  });
});
