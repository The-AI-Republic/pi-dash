// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero models (parity/specs/cycles.md §15, CYC-041): groups, the
// closed summary, the pending line, the burn-down series, chart
// availability, breakdown entries, and tab memory.
import { describe, expect, it } from "vitest";

import {
  HERO_BREAKDOWN_TABS,
  heroAssigneeEntries,
  heroBurndownPoints,
  heroCancelledLine,
  heroChartAvailable,
  heroClosedSummary,
  heroLabelEntries,
  heroMeasureAvailable,
  heroPendingCount,
  heroPriorityEntries,
  heroProgressGroups,
  heroTabStorageKey,
  parseHeroTab,
} from "./cycleHero.js";

function progress(overrides = {}) {
  return {
    total_issues: 13,
    backlog_issues: 4,
    unstarted_issues: 0,
    started_issues: 8,
    completed_issues: 1,
    cancelled_issues: 0,
    ...overrides,
  };
}

describe("heroProgressGroups", () => {
  it("lists all five groups in lifecycle order, zeros included", () => {
    expect(heroProgressGroups(progress())).toEqual([
      { id: "backlog", count: 4 },
      { id: "unstarted", count: 0 },
      { id: "started", count: 8 },
      { id: "completed", count: 1 },
      { id: "cancelled", count: 0 },
    ]);
  });
});

describe("heroClosedSummary", () => {
  it("counts closed over the total minus cancelled", () => {
    expect(heroClosedSummary(progress())).toBe("1/13 Work items closed");
    expect(heroClosedSummary(progress({ total_issues: 14, completed_issues: 1, cancelled_issues: 1 }))).toBe(
      "2/13 Work items closed"
    );
  });

  it("floors a negative denominator", () => {
    expect(heroClosedSummary(progress({ total_issues: 0, completed_issues: 0, cancelled_issues: 0 }))).toBe(
      "0/0 Work items closed"
    );
  });
});

describe("heroCancelledLine", () => {
  it("names the cancelled count", () => {
    expect(heroCancelledLine(1)).toBe("Cancelled work items 1");
    expect(heroCancelledLine(0)).toBe("Cancelled work items 0");
  });
});

describe("heroPendingCount", () => {
  it("sums the open groups", () => {
    expect(heroPendingCount(progress())).toBe("Pending work items - 12");
    expect(heroPendingCount(progress({ backlog_issues: 0, unstarted_issues: 0, started_issues: 0 }))).toBe(
      "Pending work items - 0"
    );
  });
});

describe("heroMeasureAvailable", () => {
  it("shows for an enabled non-categories system", () => {
    expect(heroMeasureAvailable({ estimatesEnabled: true, estimateType: "points", totalEstimatePoints: 0 })).toBe(true);
    expect(heroMeasureAvailable({ estimatesEnabled: true, estimateType: "time", totalEstimatePoints: 0 })).toBe(true);
  });

  it("hides for a categories system even when enabled or carrying point data", () => {
    expect(heroMeasureAvailable({ estimatesEnabled: true, estimateType: "categories", totalEstimatePoints: 0 })).toBe(
      false
    );
    expect(heroMeasureAvailable({ estimatesEnabled: true, estimateType: "categories", totalEstimatePoints: 9 })).toBe(
      false
    );
  });

  it("shows from cycle point data when estimates are disabled", () => {
    expect(heroMeasureAvailable({ estimatesEnabled: false, estimateType: null, totalEstimatePoints: 2 })).toBe(true);
    expect(heroMeasureAvailable({ estimatesEnabled: false, estimateType: "points", totalEstimatePoints: 0 })).toBe(
      false
    );
  });

  it("hides with no enabled system, no active family, and no point data", () => {
    expect(heroMeasureAvailable({ estimatesEnabled: false, estimateType: null, totalEstimatePoints: null })).toBe(
      false
    );
  });
});

describe("heroBurndownPoints", () => {
  const chart = {
    "2026-10-03": 9,
    "2026-10-01": 12,
    "2026-10-02": 10,
    "2026-10-01_estimates": 34,
    "2026-10-02_estimates": 28,
  };

  it("sorts days ascending with their remaining counts", () => {
    expect(heroBurndownPoints(chart, "issues")).toEqual([
      { day: "2026-10-01", remaining: 12 },
      { day: "2026-10-02", remaining: 10 },
      { day: "2026-10-03", remaining: 9 },
    ]);
  });

  it("reads the estimate twins under the points measure with fallback", () => {
    expect(heroBurndownPoints(chart, "points")).toEqual([
      { day: "2026-10-01", remaining: 34 },
      { day: "2026-10-02", remaining: 28 },
      { day: "2026-10-03", remaining: 9 },
    ]);
  });

  it("reads missing counts as zero", () => {
    expect(heroBurndownPoints({ "2026-10-01": undefined }, "issues")).toEqual([{ day: "2026-10-01", remaining: 0 }]);
  });

  it("drops null future days so the chart stops at today", () => {
    expect(heroBurndownPoints({ "2026-10-01": 12, "2026-10-02": null, "2026-10-03": 9 }, "issues")).toEqual([
      { day: "2026-10-01", remaining: 12 },
      { day: "2026-10-03", remaining: 9 },
    ]);
    expect(heroBurndownPoints({ "2026-10-01": 12, "2026-10-01_estimates": null }, "points")).toEqual([
      { day: "2026-10-01", remaining: 12 },
    ]);
  });
});

describe("heroChartAvailable", () => {
  const now = new Date("2026-10-23T12:00:00Z");

  it("plots a dated cycle that has started", () => {
    expect(heroChartAvailable("2026-10-01T00:00:00Z", "2026-10-31T00:00:00Z", now)).toBe(true);
    expect(heroChartAvailable("2026-10-23T12:00:00Z", "2026-10-31T00:00:00Z", now)).toBe(true);
  });

  it("withholds undated, future-start, and unparseable cycles", () => {
    expect(heroChartAvailable(null, "2026-10-31T00:00:00Z", now)).toBe(false);
    expect(heroChartAvailable("2026-10-01T00:00:00Z", null, now)).toBe(false);
    expect(heroChartAvailable(undefined, undefined, now)).toBe(false);
    expect(heroChartAvailable("2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z", now)).toBe(false);
    expect(heroChartAvailable("not-a-date", "2026-10-31T00:00:00Z", now)).toBe(false);
  });
});

describe("heroAssigneeEntries", () => {
  const distribution = {
    assignees: [
      {
        assignee_id: "ada",
        display_name: "Ada Lovelace",
        avatar_url: "https://img.test/ada.png",
        total_issues: 3,
        total_estimates: 8,
      },
    ],
    labels: [],
    completion_chart: {},
  };

  it("reads issue or estimate counts", () => {
    expect(heroAssigneeEntries(distribution, "issues")).toEqual([
      { id: "ada", name: "Ada Lovelace", avatar: "https://img.test/ada.png", count: 3 },
    ]);
    expect(heroAssigneeEntries(distribution, "points")).toEqual([
      { id: "ada", name: "Ada Lovelace", avatar: "https://img.test/ada.png", count: 8 },
    ]);
  });

  it("blanks sparse server rows", () => {
    const sparse = {
      assignees: [
        { assignee_id: null, display_name: null, avatar_url: null, total_issues: null, total_estimates: null },
      ],
      labels: [],
      completion_chart: {},
    };
    const blank = [{ id: "", name: "", avatar: null, count: 0 }];
    expect(heroAssigneeEntries(sparse, "issues")).toEqual(blank);
    expect(heroAssigneeEntries(sparse, "points")).toEqual(blank);
  });
});

describe("heroLabelEntries", () => {
  const distribution = {
    assignees: [],
    labels: [{ label_id: "bug", label_name: "Bug", color: "#ff0000", total_issues: 2, total_estimates: 5 }],
    completion_chart: {},
  };

  it("reads names, dots, and issue or estimate counts", () => {
    expect(heroLabelEntries(distribution, "issues")).toEqual([{ id: "bug", name: "Bug", color: "#ff0000", count: 2 }]);
    expect(heroLabelEntries(distribution, "points")).toEqual([{ id: "bug", name: "Bug", color: "#ff0000", count: 5 }]);
  });

  it("blanks sparse server rows", () => {
    const sparse = {
      assignees: [],
      labels: [{ label_id: null, label_name: null, color: null, total_issues: null, total_estimates: null }],
      completion_chart: {},
    };
    const blank = [{ id: "", name: "", color: null, count: 0 }];
    expect(heroLabelEntries(sparse, "issues")).toEqual(blank);
    expect(heroLabelEntries(sparse, "points")).toEqual(blank);
  });
});

describe("heroPriorityEntries", () => {
  it("keeps urgent and high rows in server order", () => {
    const issues = [
      { id: "a", name: "Low chore", priority: "low" },
      { id: "b", name: "Urgent fix", priority: "urgent" },
      { id: "c", name: "Medium task", priority: "medium" },
      { id: "d", name: "High value", priority: "high" },
      { id: "e", name: "No priority", priority: "none" },
    ] as const;
    expect(heroPriorityEntries(issues)).toEqual([
      { id: "b", name: "Urgent fix", priority: "urgent" },
      { id: "d", name: "High value", priority: "high" },
    ]);
  });
});

describe("breakdown tabs", () => {
  it("pins the tab order and labels", () => {
    expect(HERO_BREAKDOWN_TABS.map((tab) => tab.label)).toEqual(["Priority work items", "Assignees", "Labels"]);
  });

  it("keys tab memory per cycle", () => {
    expect(heroTabStorageKey("cycle-1")).toBe("cycle-hero-tab-cycle-1");
  });

  it("falls back to the priority tab on anything unkept", () => {
    expect(parseHeroTab("assignees")).toBe("assignees");
    expect(parseHeroTab("labels")).toBe("labels");
    expect(parseHeroTab("priority")).toBe("priority");
    expect(parseHeroTab("mystery")).toBe("priority");
    expect(parseHeroTab(null)).toBe("priority");
    expect(parseHeroTab(undefined)).toBe("priority");
  });
});
