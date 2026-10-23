// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-insights picker model tests (parity/specs/analytics.md §10, ANL-010).
import { describe, expect, it } from "vitest";

import {
  CUSTOM_AXES,
  CUSTOM_METRICS,
  CUSTOM_SPLIT_CLEAR_LABEL,
  CUSTOM_SPLIT_EMPTY_LABEL,
  customAxisLabel,
  customMetricLabel,
  DEFAULT_CUSTOM_METRIC,
  DEFAULT_CUSTOM_SPLIT,
  DEFAULT_CUSTOM_X_AXIS,
  isCapitalizedCustomAxis,
  isCustomDateAxis,
  selectCustomSplitOptions,
  selectCustomXOptions,
} from "./customAxes.js";

describe("custom picker catalogue", () => {
  it("offers twelve dimensions in the fixed picker order", () => {
    expect(CUSTOM_AXES.map((entry) => entry.label)).toEqual([
      "State name",
      "State group",
      "Priority",
      "Label",
      "Assignee",
      "Estimate point",
      "Cycle",
      "Module",
      "Completed date",
      "Due date",
      "Start date",
      "Created date",
    ]);
    expect(CUSTOM_AXES.map((entry) => entry.value)).toEqual([
      "STATES",
      "STATE_GROUPS",
      "PRIORITY",
      "LABELS",
      "ASSIGNEES",
      "ESTIMATE_POINTS",
      "CYCLES",
      "MODULES",
      "COMPLETED_AT",
      "TARGET_DATE",
      "START_DATE",
      "CREATED_AT",
    ]);
  });

  it("settles the metric on the single in-scope count", () => {
    expect(CUSTOM_METRICS).toEqual([{ label: "Work item", value: "WORK_ITEM_COUNT" }]);
    expect(DEFAULT_CUSTOM_METRIC).toBe("WORK_ITEM_COUNT");
    expect(DEFAULT_CUSTOM_X_AXIS).toBe("PRIORITY");
    expect(DEFAULT_CUSTOM_SPLIT).toBeNull();
    expect(CUSTOM_SPLIT_CLEAR_LABEL).toBe("No value");
    expect(CUSTOM_SPLIT_EMPTY_LABEL).toBe("Add Property");
  });

  it("classifies date and capitalized axes", () => {
    const dates = ["COMPLETED_AT", "TARGET_DATE", "START_DATE", "CREATED_AT"] as const;
    expect(dates.every((axis) => isCustomDateAxis(axis))).toBe(true);
    expect(isCustomDateAxis("PRIORITY")).toBe(false);
    expect(isCapitalizedCustomAxis("PRIORITY")).toBe(true);
    expect(isCapitalizedCustomAxis("STATE_GROUPS")).toBe(true);
    expect(isCapitalizedCustomAxis("LABELS")).toBe(false);
  });

  it("labels axes, metrics, and the empty split", () => {
    expect(customAxisLabel("TARGET_DATE")).toBe("Due date");
    expect(customAxisLabel(null)).toBe("Add Property");
    expect(customMetricLabel("WORK_ITEM_COUNT")).toBe("Work item");
  });

  it("reads unknown values verbatim", () => {
    expect(customAxisLabel("CREATED_BY" as never)).toBe("CREATED_BY");
    expect(customMetricLabel("ESTIMATE_SUM" as never)).toBe("ESTIMATE_SUM");
  });
});

describe("custom picker exclusion", () => {
  it("offers every dimension on x while no split is chosen", () => {
    expect(selectCustomXOptions(null).map((entry) => entry.label)).toEqual(CUSTOM_AXES.map((entry) => entry.label));
  });

  it("removes the active split from the x options", () => {
    expect(selectCustomXOptions("STATES").map((entry) => entry.value)).not.toContain("STATES");
    expect(selectCustomXOptions("STATES")).toHaveLength(CUSTOM_AXES.length - 1);
  });

  it("offers the clear row plus every dimension but x on the split", () => {
    const options = selectCustomSplitOptions("PRIORITY");
    expect(options[0]).toEqual({ label: "No value", value: null });
    expect(options.slice(1).map((entry) => entry.label)).toEqual(
      CUSTOM_AXES.map((entry) => entry.label).filter((label) => label !== "Priority")
    );
  });

  it("swaps the exclusion when x moves", () => {
    const options = selectCustomSplitOptions("LABELS");
    expect(options.map((entry) => entry.label)).toContain("Priority");
    expect(options.map((entry) => entry.label)).not.toContain("Label");
  });
});
