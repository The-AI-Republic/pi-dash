// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar model: the collapse flag, the per-cycle breakdown tab, the
// progress counts and shares, the chart gate, the burndown points, the
// Points gate, and the breakdown-entry filter toggle.

import { describe, expect, it } from "vitest";
import type { CycleDistribution, CycleProgress } from "@pidash/api-client";

import type { RichCondition } from "../../issues/filters/rich/expression.js";
import {
  BREAKDOWN_FILTER_STEMS,
  breakdownEntryActive,
  breakdownTabKey,
  completionChartPoints,
  CYCLE_BREAKDOWN_TABS,
  CYCLE_PROGRESS_EMPTY_TEXT,
  CYCLE_PROGRESS_MEASURES,
  CYCLE_SIDEBAR_COLLAPSED_KEY,
  cycleProgressChartVisible,
  cycleProgressEmptyText,
  DEFAULT_BREAKDOWN_TAB,
  measureLabel,
  pointsPairText,
  progressGrandTotal,
  progressShare,
  readBreakdownTab,
  readSidebarCollapsed,
  selectAssigneeRows,
  selectLabelRows,
  selectStateGroupRows,
  showProgressMeasure,
  showPointsRow,
  toggleBreakdownCondition,
  workItemsPairText,
  writeBreakdownTab,
  writeSidebarCollapsed,
  type SidebarFlagStorage,
} from "./cycleSidebar.js";

function memoryStorage(entries: Record<string, string> = {}): SidebarFlagStorage {
  const map = new Map(Object.entries(entries));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  };
}

function throwingStorage(): SidebarFlagStorage {
  return {
    getItem: () => {
      throw new Error("no storage");
    },
    setItem: () => {
      throw new Error("no storage");
    },
    removeItem: () => {
      throw new Error("no storage");
    },
  };
}

function makeProgress(overrides: Partial<CycleProgress> = {}): CycleProgress {
  return {
    total_issues: 3,
    backlog_issues: 1,
    unstarted_issues: 1,
    started_issues: 0,
    completed_issues: 1,
    cancelled_issues: 0,
    backlog_estimate_points: 2,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    completed_estimate_points: 0,
    cancelled_estimate_points: 0,
    total_estimate_points: 2,
    ...overrides,
  };
}

function makeDistribution(overrides: Partial<CycleDistribution> = {}): CycleDistribution {
  return {
    assignees: [
      {
        display_name: "Ada",
        assignee_id: "aaaaaaaa-0000-4000-8000-000000000001",
        total_issues: 2,
        total_estimates: 4,
      },
      { display_name: null, assignee_id: null, total_issues: 1, total_estimates: null },
    ],
    labels: [
      {
        label_name: "Bug",
        label_id: "bbbbbbbb-0000-4000-8000-000000000002",
        total_issues: 1,
        total_estimates: 2,
      },
      { label_name: null, label_id: null },
    ],
    completion_chart: {},
    ...overrides,
  };
}

function condition(overrides: Partial<RichCondition> = {}): RichCondition {
  return {
    key: "condition-1",
    property: "state_group",
    operator: "in",
    negated: false,
    value: ["backlog"],
    ...overrides,
  };
}

describe("sidebar collapse flag", () => {
  it("reads collapsed only for the exact true string", () => {
    expect(readSidebarCollapsed(memoryStorage({ [CYCLE_SIDEBAR_COLLAPSED_KEY]: "true" }))).toBe(true);
    expect(readSidebarCollapsed(memoryStorage({ [CYCLE_SIDEBAR_COLLAPSED_KEY]: "false" }))).toBe(false);
    expect(readSidebarCollapsed(memoryStorage())).toBe(false);
    expect(readSidebarCollapsed(throwingStorage())).toBe(false);
  });

  it("stores collapse and drops the key on expand", () => {
    const storage = memoryStorage();
    writeSidebarCollapsed(storage, true);
    expect(storage.getItem(CYCLE_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    writeSidebarCollapsed(storage, false);
    expect(storage.getItem(CYCLE_SIDEBAR_COLLAPSED_KEY)).toBe(null);
  });

  it("swallows storage failures on write", () => {
    expect(() => writeSidebarCollapsed(throwingStorage(), true)).not.toThrow();
    expect(() => writeSidebarCollapsed(throwingStorage(), false)).not.toThrow();
  });
});

describe("breakdown tab", () => {
  it("pins the tab order and the default", () => {
    expect([...CYCLE_BREAKDOWN_TABS]).toEqual(["States", "Assignees", "Labels"]);
    expect(DEFAULT_BREAKDOWN_TAB).toBe("Assignees");
    expect(breakdownTabKey("cycle-1")).toBe("cycle-analytics-tab-cycle-1");
  });

  it("reads the stored tab per cycle", () => {
    const storage = memoryStorage({
      "cycle-analytics-tab-a": "Labels",
      "cycle-analytics-tab-b": "States",
      "cycle-analytics-tab-d": "Assignees",
    });
    expect(readBreakdownTab(storage, "a")).toBe("Labels");
    expect(readBreakdownTab(storage, "b")).toBe("States");
    expect(readBreakdownTab(storage, "c")).toBe("Assignees");
    expect(readBreakdownTab(storage, "d")).toBe("Assignees");
  });

  it("falls back for foreign values and broken storage", () => {
    expect(readBreakdownTab(memoryStorage({ "cycle-analytics-tab-a": "Priority" }), "a")).toBe("Assignees");
    expect(readBreakdownTab(throwingStorage(), "a")).toBe("Assignees");
  });

  it("remembers the tab and survives write failures", () => {
    const storage = memoryStorage();
    writeBreakdownTab(storage, "a", "Labels");
    expect(storage.getItem("cycle-analytics-tab-a")).toBe("Labels");
    expect(() => writeBreakdownTab(throwingStorage(), "a", "Labels")).not.toThrow();
  });
});

describe("measure", () => {
  it("pins the measure options and labels", () => {
    expect([...CYCLE_PROGRESS_MEASURES]).toEqual([
      { value: "issues", label: "Work items" },
      { value: "points", label: "Estimates" },
    ]);
    expect(measureLabel("issues")).toBe("Work items");
    expect(measureLabel("points")).toBe("Estimates");
  });
});

describe("progress totals and shares", () => {
  it("totals include cancelled items on both measures", () => {
    const progress = makeProgress({ total_issues: 4, cancelled_issues: 1, total_estimate_points: 5 });
    expect(progressGrandTotal(progress, "issues")).toBe(4);
    expect(progressGrandTotal(progress, "points")).toBe(5);
    expect(progressGrandTotal(makeProgress({ total_estimate_points: null }), "points")).toBe(0);
  });

  it("shares round with a zero-safe floor", () => {
    expect(progressShare(1, 3)).toBe(33);
    expect(progressShare(2, 3)).toBe(67);
    expect(progressShare(1, 0)).toBe(0);
    expect(progressShare(0, 3)).toBe(0);
    expect(progressShare(-1, 3)).toBe(0);
  });
});

describe("state group rows", () => {
  it("counts items with the cancelled row only when it holds items", () => {
    expect(selectStateGroupRows(makeProgress(), "issues")).toEqual([
      { key: "backlog", title: "Backlog", count: 1, total: 3, percent: 33 },
      { key: "unstarted", title: "Unstarted", count: 1, total: 3, percent: 33 },
      { key: "started", title: "Started", count: 0, total: 3, percent: 0 },
      { key: "completed", title: "Completed", count: 1, total: 3, percent: 33 },
    ]);
    const rows = selectStateGroupRows(makeProgress({ total_issues: 4, cancelled_issues: 1 }), "issues");
    expect(rows.at(-1)).toEqual({ key: "cancelled", title: "Cancelled", count: 1, total: 4, percent: 25 });
  });

  it("counts estimate points on the estimates measure", () => {
    const rows = selectStateGroupRows(makeProgress(), "points");
    expect(rows.find((row) => row.key === "backlog")).toEqual({
      key: "backlog",
      title: "Backlog",
      count: 2,
      total: 2,
      percent: 100,
    });
    expect(rows.find((row) => row.key === "completed")).toEqual({
      key: "completed",
      title: "Completed",
      count: 0,
      total: 2,
      percent: 0,
    });
  });

  it("counts cancelled estimate points on the estimates measure", () => {
    const rows = selectStateGroupRows(
      makeProgress({ cancelled_estimate_points: 1, total_estimate_points: 3 }),
      "points"
    );
    expect(rows.at(-1)).toEqual({ key: "cancelled", title: "Cancelled", count: 1, total: 3, percent: 33 });
  });
});

describe("distribution rows", () => {
  it("reads assignee entries with fallbacks", () => {
    expect(selectAssigneeRows(makeDistribution(), "issues", 3)).toEqual([
      {
        id: "aaaaaaaa-0000-4000-8000-000000000001",
        title: "Ada",
        count: 2,
        total: 3,
        percent: 67,
      },
      { id: null, title: "Unassigned", count: 1, total: 3, percent: 33 },
    ]);
  });

  it("reads assignee estimates with missing counts as zero", () => {
    const rows = selectAssigneeRows(makeDistribution(), "points", 4);
    expect(rows[0]).toEqual({
      id: "aaaaaaaa-0000-4000-8000-000000000001",
      title: "Ada",
      count: 4,
      total: 4,
      percent: 100,
    });
    expect(rows[1]).toEqual({ id: null, title: "Unassigned", count: 0, total: 4, percent: 0 });
  });

  it("reads label entries with fallbacks", () => {
    expect(selectLabelRows(makeDistribution(), "issues", 3)).toEqual([
      {
        id: "bbbbbbbb-0000-4000-8000-000000000002",
        title: "Bug",
        count: 1,
        total: 3,
        percent: 33,
      },
      { id: null, title: "Unlabeled", count: 0, total: 3, percent: 0 },
    ]);
  });

  it("reads broken counts as zero", () => {
    const rows = selectAssigneeRows(
      makeDistribution({ assignees: [{ display_name: "Zed", assignee_id: null, total_issues: NaN }] }),
      "issues",
      1
    );
    expect(rows).toEqual([{ id: null, title: "Zed", count: 0, total: 1, percent: 0 }]);
  });

  it("reads label estimates", () => {
    const rows = selectLabelRows(makeDistribution(), "points", 2);
    expect(rows[0]).toEqual({
      id: "bbbbbbbb-0000-4000-8000-000000000002",
      title: "Bug",
      count: 2,
      total: 2,
      percent: 100,
    });
  });
});

describe("chart gate", () => {
  const NOW = new Date("2026-10-10T12:00:00Z");

  it("shows the chart for a started range", () => {
    expect(cycleProgressChartVisible("2026-09-01T00:00:00Z", "2026-12-31T00:00:00Z", "UTC", NOW)).toBe(true);
    expect(cycleProgressChartVisible("2026-10-10T00:00:00Z", "2026-12-31T00:00:00Z", "UTC", NOW)).toBe(true);
  });

  it("hides the chart without a valid past start", () => {
    expect(cycleProgressChartVisible(null, null, "UTC", NOW)).toBe(false);
    expect(cycleProgressChartVisible("2026-09-01T00:00:00Z", null, "UTC", NOW)).toBe(false);
    expect(cycleProgressChartVisible(null, "2026-12-31T00:00:00Z", "UTC", NOW)).toBe(false);
    expect(cycleProgressChartVisible("2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z", "UTC", NOW)).toBe(false);
    expect(cycleProgressChartVisible("not-a-date", "2026-12-31T00:00:00Z", "UTC", NOW)).toBe(false);
  });

  it("shows the empty notice only for undated cycles", () => {
    expect(CYCLE_PROGRESS_EMPTY_TEXT).toBe("No Data yet");
    expect(cycleProgressEmptyText(null, null)).toBe("No Data yet");
    expect(cycleProgressEmptyText("2026-09-01T00:00:00Z", null)).toBe(null);
    expect(cycleProgressEmptyText(null, "2026-12-31T00:00:00Z")).toBe(null);
    expect(cycleProgressEmptyText("2026-09-01T00:00:00Z", "2026-12-31T00:00:00Z")).toBe(null);
  });
});

describe("burndown points", () => {
  it("orders dated readings and drops future nulls", () => {
    expect(
      completionChartPoints({
        "2026-10-03": 1,
        "2026-10-01": 3,
        "2026-10-02": 2,
        "2026-10-20": null,
      })
    ).toEqual([
      { label: "Oct 1, 2026", values: { remaining: 3 } },
      { label: "Oct 2, 2026", values: { remaining: 2 } },
      { label: "Oct 3, 2026", values: { remaining: 1 } },
    ]);
  });

  it("drops non-numeric readings and reads empty charts as no points", () => {
    expect(completionChartPoints({ "2026-10-01": "3" as unknown as number })).toEqual([]);
    expect(completionChartPoints({})).toEqual([]);
  });
});

describe("count pairs", () => {
  it("pairs completed over total with the zero-total string", () => {
    expect(workItemsPairText(makeProgress())).toBe("1/3");
    expect(workItemsPairText(makeProgress({ completed_issues: 0, total_issues: 2 }))).toBe("0/2");
    expect(workItemsPairText(makeProgress({ total_issues: 0 }))).toBe("0 Work item");
    expect(workItemsPairText(makeProgress({ total_issues: -1 }))).toBe("0 Work item");
  });

  it("pairs estimate points with a zero-safe total", () => {
    expect(pointsPairText(makeProgress())).toBe("0/2");
    expect(pointsPairText(makeProgress({ completed_estimate_points: 1, total_estimate_points: 3 }))).toBe("1/3");
    expect(pointsPairText(makeProgress({ total_estimate_points: null }))).toBe("0/0");
  });
});

describe("points row gate", () => {
  const base = { status: "current", totalEstimatePoints: 2, snapshot: {} };

  it("shows for points systems on open cycles", () => {
    expect(showPointsRow({ ...base, estimateType: "points" })).toBe(true);
  });

  it("hides without a points system or estimate totals", () => {
    expect(showPointsRow({ ...base, estimateType: "time" })).toBe(false);
    expect(showPointsRow({ ...base, estimateType: null })).toBe(false);
    expect(showPointsRow({ ...base, estimateType: "points", totalEstimatePoints: null })).toBe(false);
  });

  it("gates finished cycles on the snapshot distribution", () => {
    expect(
      showPointsRow({
        ...base,
        estimateType: "points",
        status: "completed",
        snapshot: { progress_snapshot: { estimate_distribution: {} } },
      })
    ).toBe(true);
    expect(
      showPointsRow({ ...base, estimateType: "points", status: "completed", snapshot: { progress_snapshot: {} } })
    ).toBe(false);
    expect(
      showPointsRow({
        ...base,
        estimateType: "points",
        status: "completed",
        snapshot: { progress_snapshot: { estimate_distribution: null } },
      })
    ).toBe(false);
    expect(
      showPointsRow({ ...base, estimateType: "points", status: "completed", snapshot: { progress_snapshot: null } })
    ).toBe(false);
    expect(showPointsRow({ ...base, estimateType: "points", status: "completed", snapshot: {} })).toBe(false);
  });
});

describe("progress measure gate", () => {
  it("shows for an enabled points system", () => {
    expect(showProgressMeasure({ estimatesEnabled: true, estimateType: "points", progress: makeProgress() })).toBe(
      true
    );
  });

  it("shows for cycle point data even without an enabled system", () => {
    expect(
      showProgressMeasure({
        estimatesEnabled: false,
        estimateType: null,
        progress: makeProgress({ total_estimate_points: 2 }),
      })
    ).toBe(true);
  });

  it("hides for a categories system even when estimates are enabled", () => {
    expect(showProgressMeasure({ estimatesEnabled: true, estimateType: "categories", progress: makeProgress() })).toBe(
      false
    );
  });

  it("shows for an enabled time system", () => {
    expect(showProgressMeasure({ estimatesEnabled: true, estimateType: "time", progress: makeProgress() })).toBe(true);
  });

  it("shows for point data without an enabled system on any non-categories family", () => {
    expect(
      showProgressMeasure({
        estimatesEnabled: false,
        estimateType: "time",
        progress: makeProgress({ total_estimate_points: 2 }),
      })
    ).toBe(true);
  });

  it("hides with no system and no point data", () => {
    expect(
      showProgressMeasure({
        estimatesEnabled: false,
        estimateType: null,
        progress: makeProgress({ total_estimate_points: 0 }),
      })
    ).toBe(false);
    expect(
      showProgressMeasure({
        estimatesEnabled: false,
        estimateType: null,
        progress: makeProgress({ total_estimate_points: null }),
      })
    ).toBe(false);
  });
});

describe("breakdown filter toggle", () => {
  it("pins the stem behind each tab", () => {
    expect(BREAKDOWN_FILTER_STEMS).toEqual({ States: "state_group", Assignees: "assignee_id", Labels: "label_id" });
  });

  it("leaves unknown stems untouched", () => {
    const before = [condition()];
    expect(toggleBreakdownCondition(before, "nope", "x")).toEqual(before);
  });

  it("adds a first value as a new condition", () => {
    const next = toggleBreakdownCondition([], "state_group", "backlog");
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ property: "state_group", operator: "in", negated: false, value: ["backlog"] });
    expect(typeof next[0]!.key).toBe("string");
  });

  it("combines a second value into the stem condition", () => {
    const next = toggleBreakdownCondition([condition()], "state_group", "completed");
    expect(next).toHaveLength(1);
    expect(next[0]!.value).toEqual(["backlog", "completed"]);
  });

  it("lifts a repeat click and drops the emptied condition", () => {
    const combined = toggleBreakdownCondition([condition()], "state_group", "completed");
    const lifted = toggleBreakdownCondition(combined, "state_group", "backlog");
    expect(lifted).toHaveLength(1);
    expect(lifted[0]!.value).toEqual(["completed"]);
    expect(toggleBreakdownCondition(lifted, "state_group", "completed")).toEqual([]);
  });

  it("keeps other conditions and stems untouched", () => {
    const other = condition({ key: "condition-9", property: "priority", operator: "in", value: ["high"] });
    const next = toggleBreakdownCondition([other, condition()], "state_group", "backlog");
    expect(next).toEqual([other]);
  });

  it("combines into the stem condition beside others", () => {
    const other = condition({ key: "condition-9", property: "priority", operator: "in", value: ["high"] });
    const next = toggleBreakdownCondition([other, condition()], "state_group", "completed");
    expect(next).toHaveLength(2);
    expect(next[0]).toEqual(other);
    expect(next[1]!.value).toEqual(["backlog", "completed"]);
  });

  it("adds alongside a negated or foreign-operator stem condition", () => {
    const negated = condition({ key: "condition-2", negated: true });
    const next = toggleBreakdownCondition([negated], "state_group", "completed");
    expect(next).toHaveLength(2);
    expect(next[0]).toEqual(negated);
    expect(next[1]).toMatchObject({ property: "state_group", value: ["completed"] });
    const exact = condition({ key: "condition-3", operator: "exact", value: "backlog" });
    expect(toggleBreakdownCondition([exact], "state_group", "completed")).toHaveLength(2);
  });

  it("toggles a scalar stem value", () => {
    const scalar = condition({ key: "condition-4", value: "backlog" });
    expect(toggleBreakdownCondition([scalar], "state_group", "backlog")).toEqual([]);
  });

  it("reports whether an entry is applied", () => {
    expect(breakdownEntryActive([condition()], "state_group", "backlog")).toBe(true);
    expect(breakdownEntryActive([condition()], "state_group", "completed")).toBe(false);
    expect(breakdownEntryActive([], "state_group", "backlog")).toBe(false);
    const negated = condition({ key: "condition-5", negated: true });
    expect(breakdownEntryActive([negated], "state_group", "backlog")).toBe(false);
    const exact = condition({ key: "condition-6", operator: "exact", value: "backlog" });
    expect(breakdownEntryActive([exact], "state_group", "backlog")).toBe(false);
  });
});
