// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { State } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { state } from "./testFixtures.js";
import {
  canDragGroupRows,
  COLLAPSED_ON_LOAD,
  compareStateSequence,
  groupStates,
  isWorkflowGroup,
  rowShare,
  WORKFLOW_GROUPS,
  WORKFLOW_GROUP_LABELS,
  WORKFLOW_GROUP_SWATCHES,
} from "./groups.js";

describe("workflow groups", () => {
  it("names exactly seven groups in display order with no triage", () => {
    expect(WORKFLOW_GROUPS).toEqual(["backlog", "unstarted", "started", "review", "test", "completed", "cancelled"]);
    expect(WORKFLOW_GROUPS).not.toContain("triage");
    for (const group of WORKFLOW_GROUPS) {
      expect(WORKFLOW_GROUP_LABELS[group]).not.toBe("");
      expect(WORKFLOW_GROUP_SWATCHES[group]).toMatch(/^#[0-9A-F]{6}$/);
    }
  });

  it("labels each group", () => {
    expect(WORKFLOW_GROUP_LABELS.backlog).toBe("Backlog");
    expect(WORKFLOW_GROUP_LABELS.cancelled).toBe("Cancelled");
  });

  it("starts collapsed on review only", () => {
    expect(COLLAPSED_ON_LOAD).toEqual(["review"]);
  });

  it("recognises the seven groups and rejects triage or unknown values", () => {
    expect(isWorkflowGroup("backlog")).toBe(true);
    expect(isWorkflowGroup("cancelled")).toBe(true);
    expect(isWorkflowGroup("triage")).toBe(false);
    expect(isWorkflowGroup("nope")).toBe(false);
  });
});

describe("groupStates", () => {
  it("renders all seven cards even when no state belongs to a group", () => {
    const grouped = groupStates([]);
    expect(grouped.map((entry) => entry.group)).toEqual([...WORKFLOW_GROUPS]);
    for (const entry of grouped) expect(entry.rows).toEqual([]);
  });

  it("buckets rows and sorts them by sequence inside their group", () => {
    const grouped = groupStates([
      state({ name: "b-late", group: "backlog", sequence: 30 }),
      state({ name: "started", group: "started", sequence: 1 }),
      state({ name: "b-early", group: "backlog", sequence: 10 }),
      state({ name: "b-mid", group: "backlog", sequence: 20 }),
    ]);
    const backlog = grouped.find((entry) => entry.group === "backlog");
    expect(backlog?.rows.map((row) => row.name)).toEqual(["b-early", "b-mid", "b-late"]);
    expect(backlog?.label).toBe("Backlog");
    expect(grouped.find((entry) => entry.group === "started")?.rows).toHaveLength(1);
  });

  it("drops rows outside the fixed set instead of inventing a card", () => {
    const stray = { ...state({ name: "stray", group: "backlog" }), group: "triage" } as unknown as State;
    const grouped = groupStates([stray, state({ name: "real", group: "test" })]);
    expect(grouped.map((entry) => entry.group)).toEqual([...WORKFLOW_GROUPS]);
    expect(grouped.flatMap((entry) => entry.rows.map((row) => row.name))).toEqual(["real"]);
  });

  it("keeps equal sequence values in their incoming order", () => {
    const first = state({ name: "first", group: "test", sequence: 5 });
    const second = state({ name: "second", group: "test", sequence: 5 });
    expect(groupStates([first, second]).find((entry) => entry.group === "test")?.rows).toEqual([first, second]);
  });

  it("does not mutate the caller's list", () => {
    const rows = [
      state({ name: "late", group: "test", sequence: 9 }),
      state({ name: "early", group: "test", sequence: 1 }),
    ];
    groupStates(rows);
    expect(rows.map((row) => row.name)).toEqual(["late", "early"]);
  });
});

describe("compareStateSequence", () => {
  it("orders ascending", () => {
    expect(compareStateSequence({ sequence: 1 }, { sequence: 2 })).toBeLessThan(0);
    expect(compareStateSequence({ sequence: 2 }, { sequence: 1 })).toBeGreaterThan(0);
    expect(compareStateSequence({ sequence: 3 }, { sequence: 3 })).toBe(0);
  });
});

describe("canDragGroupRows", () => {
  it("needs editing and more than one row", () => {
    expect(canDragGroupRows(2, true)).toBe(true);
    expect(canDragGroupRows(1, true)).toBe(false);
    expect(canDragGroupRows(2, false)).toBe(false);
    expect(canDragGroupRows(0, false)).toBe(false);
  });
});

describe("rowShare", () => {
  it("is the 1-based position over the group size", () => {
    expect(rowShare(1, 4)).toBe(0.25);
    expect(rowShare(4, 4)).toBe(1);
  });

  it("reads zero for a row outside any group", () => {
    expect(rowShare(1, 0)).toBe(0);
    expect(rowShare(0, -2)).toBe(0);
  });
});
