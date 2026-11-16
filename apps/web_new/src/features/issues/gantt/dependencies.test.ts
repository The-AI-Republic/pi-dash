// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dependency shaping (ISS-237): which rows a link may use, how a row's
// finish-to-start relations become edges, where an arrow starts and ends,
// and which edges the timeline can actually draw.
import { describe, expect, it } from "vitest";

import type { IssueRelations } from "@pidash/api-client";

import {
  ARROW_MARKER_ID,
  dependencyEligibleIds,
  dependencyEnds,
  dependencyPath,
  drawableEdges,
  edgesOf,
  layerHeight,
  laneCentre,
  mergeEdges,
  type DependencyEdge,
  type DependencyEnd,
} from "./dependencies.js";
import { ROW_HEIGHT, type GanttRow } from "./rows.js";
import type { GanttRange } from "./scale.js";

const RANGE: GanttRange = { startIso: "2026-10-01", dayCount: 30 };

function row(overrides: Partial<GanttRow> = {}): GanttRow {
  return {
    id: "row-1",
    identifier: "WEB-1",
    name: "First",
    startIso: "2026-10-01",
    targetIso: "2026-10-03",
    durationDays: 3,
    stateId: null,
    sortOrder: 1,
    priority: "none",
    sequenceId: 1,
    archived: false,
    unconfirmed: false,
    isEpic: false,
    ...overrides,
  };
}

function end(laneIndex: number, left: number, width = 40): DependencyEnd {
  return { box: { left, width }, laneTop: laneIndex * ROW_HEIGHT, laneHeight: ROW_HEIGHT };
}

function relations(finishAfterIds: string[]): IssueRelations {
  const related = (id: string) => ({
    id,
    name: "row",
    state_id: null,
    sort_order: 1,
    priority: "none" as const,
    sequence_id: 1,
    project_id: "323e4567-e89b-12d3-a456-426614174002",
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    relation_type: "finish_after" as const,
  });
  return {
    blocking: [],
    blocked_by: [],
    duplicate: [],
    relates_to: [],
    start_after: [],
    start_before: [],
    finish_after: finishAfterIds.map(related),
    finish_before: [],
  };
}

describe("dependencyEligibleIds", () => {
  it("keeps confirmed rows and drops an unconfirmed create", () => {
    expect([...dependencyEligibleIds([row(), row({ id: "row-2", unconfirmed: true })])]).toEqual(["row-1"]);
  });
});

describe("edgesOf", () => {
  it("turns every finish-after row into an edge onto this item", () => {
    expect(edgesOf("b", relations(["a"]))).toEqual([{ fromId: "a", toId: "b" }]);
  });

  it("answers nothing without a read", () => {
    expect(edgesOf("b", undefined)).toEqual([]);
  });
});

describe("mergeEdges", () => {
  it("deduplicates the same edge read twice", () => {
    expect(mergeEdges([[{ fromId: "a", toId: "b" }], [{ fromId: "a", toId: "b" }]])).toEqual([
      { fromId: "a", toId: "b" },
    ]);
  });

  it("drops a self link", () => {
    expect(mergeEdges([[{ fromId: "a", toId: "a" }]])).toEqual([]);
  });

  it("keeps both directions of a pair", () => {
    expect(
      mergeEdges([
        [
          { fromId: "a", toId: "b" },
          { fromId: "b", toId: "a" },
        ],
      ])
    ).toHaveLength(2);
  });
});

describe("dependencyEnds", () => {
  it("gives a dated row an end at its own bar", () => {
    const ends = dependencyEnds({ rows: [row()], range: RANGE, zoom: "week", rowHeight: ROW_HEIGHT });
    expect(ends.get("row-1")).toEqual({ box: { left: 0, width: 120 }, laneTop: 0, laneHeight: ROW_HEIGHT });
  });

  it("leaves an undated row without an end", () => {
    const ends = dependencyEnds({
      rows: [row({ startIso: null, targetIso: null })],
      range: RANGE,
      zoom: "week",
      rowHeight: ROW_HEIGHT,
    });
    expect(ends.size).toBe(0);
  });

  it("places each row in its own lane", () => {
    const ends = dependencyEnds({
      rows: [row(), row({ id: "row-2", startIso: "2026-10-05" })],
      range: RANGE,
      zoom: "week",
      rowHeight: ROW_HEIGHT,
    });
    expect(ends.get("row-2")?.laneTop).toBe(ROW_HEIGHT);
    expect(ends.get("row-2")?.box.left).toBe(160);
  });
});

describe("drawableEdges", () => {
  it("keeps only edges whose both bar ends are drawn", () => {
    const ends = dependencyEnds({
      rows: [row(), row({ id: "row-1b", startIso: "2026-10-05" })],
      range: RANGE,
      zoom: "week",
      rowHeight: ROW_HEIGHT,
    });
    const edges: DependencyEdge[] = [
      { fromId: "row-1", toId: "row-1b" },
      { fromId: "row-1", toId: "missing" },
      { fromId: "missing", toId: "row-1" },
    ];
    expect(drawableEdges(edges, ends)).toEqual([{ fromId: "row-1", toId: "row-1b" }]);
  });
});

describe("dependencyPath", () => {
  it("draws a straight line between bars in the same lane", () => {
    const centre = ROW_HEIGHT + ROW_HEIGHT / 2;
    expect(dependencyPath(end(1, 0, 40), end(1, 200, 40))).toBe(`M 40 ${centre} L 198 ${centre}`);
  });

  it("turns in the gutter between lanes", () => {
    const path = dependencyPath(end(0, 0, 40), end(2, 200, 40));
    expect(path).toContain("Q");
    expect(path.startsWith("M 40 22")).toBe(true);
    expect(path.endsWith("L 198 110")).toBe(true);
  });

  it("turns upward when the successor sits in an earlier lane", () => {
    const path = dependencyPath(end(2, 0, 40), end(0, 200, 40));
    expect(path.startsWith("M 40 110")).toBe(true);
    expect(path).toContain("Q 119 110 119 104");
    expect(path).toContain("Q 119 22 125 22");
    expect(path.endsWith("L 198 22")).toBe(true);
  });

  it("drops below both lanes for a backward link", () => {
    const path = dependencyPath(end(2, 200, 40), end(0, 0, 40));
    expect(path).not.toContain("Q 120");
    expect(path.startsWith("M 240 110")).toBe(true);
    expect(path.endsWith("L -2 22")).toBe(true);
  });
});

describe("layer helpers", () => {
  it("covers every lane", () => {
    expect(layerHeight([row(), row()], ROW_HEIGHT)).toBe(ROW_HEIGHT * 2);
    expect(laneCentre(1, ROW_HEIGHT)).toBe(ROW_HEIGHT + ROW_HEIGHT / 2);
  });

  it("names the arrowhead marker the layer paints", () => {
    expect(ARROW_MARKER_ID).toBe("gantt-dependency-arrow");
  });
});
