// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview composition (parity/specs/profile.md §1, PROF-003/004).
import { describe, expect, it } from "vitest";

import type { ProfilePriorityBucket, ProfileStateBucket, UserProfileStats } from "@pidash/api-client";

import {
  PROFILE_STATE_GROUPS,
  chartsAreEmpty,
  overviewCards,
  priorityCategories,
  priorityMax,
  prioritySeries,
  stateLegend,
  stateTotal,
  workloadGroups,
} from "./stats.js";

function stats(overrides: Partial<UserProfileStats> = {}): UserProfileStats {
  return {
    state_distribution: [],
    priority_distribution: [],
    created_issues: 0,
    assigned_issues: 0,
    completed_issues: 0,
    pending_issues: 0,
    subscribed_issues: 0,
    present_cycles: [],
    upcoming_cycles: [],
    ...overrides,
  };
}

describe("overview cards", () => {
  it("renders created, assigned and subscribed totals in a fixed order", () => {
    expect(overviewCards({ created_issues: 4, assigned_issues: 2, subscribed_issues: 7 })).toEqual([
      { title: "Work items created", value: "4", tab: "created" },
      { title: "Work items assigned", value: "2", tab: "assigned" },
      { title: "Work items subscribed", value: "7", tab: "subscribed" },
    ]);
  });
});

describe("workload", () => {
  it("always renders the seven groups with absent ones as zero", () => {
    const buckets: ProfileStateBucket[] = [
      { state_group: "started", state_count: 3 },
      { state_group: "completed", state_count: 1 },
      { state_group: "mystery", state_count: 9 },
    ];
    const groups = workloadGroups(buckets);
    expect(groups.map((group) => group.group)).toEqual([...PROFILE_STATE_GROUPS]);
    expect(groups.map((group) => group.label)).toEqual([
      "Backlog",
      "Not started",
      "Working on",
      "Review",
      "Test",
      "Completed",
      "Canceled",
    ]);
    expect(groups.map((group) => group.count)).toEqual(["0", "0", "3", "0", "0", "1", "0"]);
  });

  it("zeroes every group for a member with nothing assigned", () => {
    expect(workloadGroups([]).every((group) => group.count === "0")).toBe(true);
  });
});

describe("charts", () => {
  const priority: ProfilePriorityBucket[] = [
    { priority: "low", priority_count: 2, priority_order: 3 },
    { priority: "urgent", priority_count: 5, priority_order: 0 },
  ];

  it("orders priority categories and bars by the server order", () => {
    expect(priorityCategories(priority)).toEqual(["Urgent", "Low"]);
    expect(prioritySeries(priority)).toEqual([
      { label: "Urgent", count: 5 },
      { label: "Low", count: 2 },
    ]);
    expect(priorityMax(prioritySeries(priority))).toBe(5);
    expect(priorityMax([])).toBe(0);
  });

  it("leaves the original array untouched", () => {
    const input = [...priority];
    prioritySeries(input);
    expect(input).toEqual(priority);
  });

  it("renders the seven state legend rows with zeros and cancels out when empty", () => {
    const legend = stateLegend([
      { state_group: "review", state_count: 2 },
      { state_group: "cancelled", state_count: 1 },
    ]);
    expect(legend.map((row) => row.label)).toEqual([
      "Backlog",
      "Unstarted",
      "Started",
      "Review",
      "Test",
      "Completed",
      "Canceled",
    ]);
    expect(legend.map((row) => row.count)).toEqual(["0", "0", "0", "2", "0", "0", "1"]);
    expect(stateLegend([])).toEqual([]);
  });

  it("totals the state bar", () => {
    expect(
      stateTotal([
        { state_group: "backlog", state_count: 2 },
        { state_group: "started", state_count: 3 },
      ])
    ).toBe(5);
    expect(stateTotal([])).toBe(0);
  });

  it("calls the charts empty only when there is no assigned work and no buckets", () => {
    expect(chartsAreEmpty(stats())).toBe(true);
    expect(chartsAreEmpty(stats({ assigned_issues: 1 }))).toBe(false);
    expect(chartsAreEmpty(stats({ state_distribution: [{ state_group: "started", state_count: 1 }] }))).toBe(false);
    expect(
      chartsAreEmpty(stats({ priority_distribution: [{ priority: "low", priority_count: 1, priority_order: 3 }] }))
    ).toBe(false);
  });
});
