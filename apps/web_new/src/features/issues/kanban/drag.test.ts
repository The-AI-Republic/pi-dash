// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag model (parity/specs/issues.md §5, ISS-037–041): draggable
// dimensions, the manual-order gate, insertion sort values, the field
// and bridge changes per dimension, and the refusal overlay plus notice.
import { describe, expect, it } from "vitest";

import type { IssueListItem } from "@pidash/api-client";

import {
  BOARD_DRAG_THRESHOLD_PX,
  boardDropRefusal,
  dimensionMove,
  DRAGGABLE_DIMENSIONS,
  insertionIndexForDrop,
  isDraggableDimension,
  isManualBoardOrder,
  MOVE_REFUSED_TITLE,
  neighborSortOrder,
  planBoardMove,
  SORT_ORDER_STEP,
  type BoardMoveInput,
} from "./drag.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const TODO = "323e4567-e89b-12d3-a456-426614174002";
const DONE = "423e4567-e89b-12d3-a456-426614174003";
const LABEL = "523e4567-e89b-12d3-a456-426614174004";
const MEMBER = "623e4567-e89b-12d3-a456-426614174005";
const CYCLE = "723e4567-e89b-12d3-a456-426614174006";
const MODULE = "823e4567-e89b-12d3-a456-426614174007";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Fix login",
    state_id: TODO,
    sort_order: 25000,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function moveInput(overrides: Partial<BoardMoveInput> = {}): BoardMoveInput {
  return {
    issue: row(),
    groupDimension: "state",
    subDimension: null,
    sourceColumn: TODO,
    sourceLane: null,
    destColumn: TODO,
    destLane: null,
    destRows: [],
    index: 0,
    sourceIndex: 0,
    ...overrides,
  };
}

describe("draggable dimensions", () => {
  it("lists the six dimensions cards may leave", () => {
    expect([...DRAGGABLE_DIMENSIONS].sort()).toEqual(
      ["assignees", "cycle", "labels", "module", "priority", "state"].sort()
    );
  });

  it("accepts the six dimensions and flat boards", () => {
    for (const dimension of [...DRAGGABLE_DIMENSIONS, null]) {
      expect(isDraggableDimension(dimension)).toBe(true);
    }
  });

  it("refuses project, creator and state-group dimensions", () => {
    for (const dimension of ["project", "team_project", "created_by", "state_detail.group", "target_date"]) {
      expect(isDraggableDimension(dimension)).toBe(false);
    }
  });

  it("treats only sort_order as the manual order", () => {
    expect(isManualBoardOrder("sort_order")).toBe(true);
    expect(isManualBoardOrder("-created_at")).toBe(false);
    expect(isManualBoardOrder("priority")).toBe(false);
  });

  it("starts a drag after a few pixels of travel", () => {
    expect(BOARD_DRAG_THRESHOLD_PX).toBe(4);
  });
});

describe("neighborSortOrder", () => {
  it("takes the midpoint between two neighbours", () => {
    expect(neighborSortOrder([row({ sort_order: 10000 }), row({ sort_order: 20000 })], 1)).toBe(15000);
  });

  it("steps below the first card", () => {
    expect(neighborSortOrder([row({ sort_order: 100000 })], 0)).toBe(100000 - SORT_ORDER_STEP);
  });

  it("steps above the last card", () => {
    expect(neighborSortOrder([row({ sort_order: 10000 })], 1)).toBe(10000 + SORT_ORDER_STEP);
  });

  it("starts an empty column at the default step", () => {
    expect(neighborSortOrder([], 0)).toBe(SORT_ORDER_STEP);
  });
});

describe("dimensionMove", () => {
  it("returns no change without a dimension or on the same key", () => {
    expect(dimensionMove(null, TODO, DONE, row())).toEqual({ patch: {}, cycle: null, modules: null });
    expect(dimensionMove("state", TODO, TODO, row())).toEqual({ patch: {}, cycle: null, modules: null });
  });

  it("replaces scalar fields and clears them on the empty column", () => {
    expect(dimensionMove("state", TODO, DONE, row()).patch).toEqual({ state_id: DONE });
    expect(dimensionMove("state", TODO, "None", row()).patch).toEqual({ state_id: null });
    expect(dimensionMove("priority", "none", "high", row()).patch).toEqual({ priority: "high" });
    expect(dimensionMove("priority", "high", "None", row()).patch).toEqual({ priority: "none" });
    expect(dimensionMove("priority", "high", "none", row()).patch).toEqual({ priority: "none" });
  });

  it("ignores unknown priority values", () => {
    expect(dimensionMove("priority", "high", "someday", row())).toEqual({
      patch: {},
      cycle: null,
      modules: null,
    });
  });

  it("moves list memberships across columns and clears on the empty column", () => {
    const labeled = row({ label_ids: [LABEL], assignee_ids: [MEMBER] });
    expect(dimensionMove("labels", LABEL, "other-label", labeled).patch).toEqual({
      label_ids: ["other-label"],
    });
    expect(dimensionMove("labels", LABEL, "None", labeled).patch).toEqual({ label_ids: [] });
    expect(dimensionMove("labels", "None", LABEL, labeled).patch).toEqual({ label_ids: [LABEL] });
    expect(dimensionMove("assignees", MEMBER, "None", labeled).patch).toEqual({ assignee_ids: [] });
  });

  it("keeps the other list members a move does not touch", () => {
    const labeled = row({ label_ids: [LABEL, "kept"] });
    expect(dimensionMove("labels", LABEL, "next", labeled).patch).toEqual({ label_ids: ["kept", "next"] });
  });

  it("adds assignees from the empty column without duplicating members", () => {
    const assigned = row({ assignee_ids: [MEMBER] });
    expect(dimensionMove("assignees", "None", "next-member", assigned).patch).toEqual({
      assignee_ids: [MEMBER, "next-member"],
    });
    const both = row({ assignee_ids: [MEMBER, "next-member"] });
    expect(dimensionMove("assignees", MEMBER, "next-member", both).patch).toEqual({ assignee_ids: ["next-member"] });
  });

  it("bridges cycle membership through add and remove", () => {
    expect(dimensionMove("cycle", "None", CYCLE, row()).cycle).toEqual({ add: CYCLE, remove: null });
    expect(dimensionMove("cycle", CYCLE, "None", row()).cycle).toEqual({ add: null, remove: CYCLE });
  });

  it("bridges module membership as add/remove pairs", () => {
    expect(dimensionMove("module", "None", MODULE, row()).modules).toEqual({ add: [MODULE], remove: [] });
    expect(dimensionMove("module", MODULE, "None", row({ module_ids: [MODULE] })).modules).toEqual({
      add: [],
      remove: [MODULE],
    });
    expect(dimensionMove("module", MODULE, "next", row({ module_ids: [MODULE] })).modules).toEqual({
      add: ["next"],
      remove: [MODULE],
    });
  });

  it("skips module writes for memberships already in place", () => {
    expect(dimensionMove("module", "gone", MODULE, row({ module_ids: [MODULE] })).modules).toEqual({
      add: [],
      remove: [],
    });
  });

  it("changes nothing for unknown dimensions", () => {
    expect(dimensionMove("created_by", "a", "b", row())).toEqual({ patch: {}, cycle: null, modules: null });
  });
});

describe("planBoardMove", () => {
  it("stays silent on a drop back onto its own seat", () => {
    const destRows = [row({ id: "a", sort_order: 10000 }), row({ id: "b", sort_order: 20000 })];
    expect(planBoardMove(moveInput({ destRows, index: 1, sourceIndex: 1 }))).toBeNull();
  });

  it("reorders within the column with a midpoint sort", () => {
    const destRows = [row({ id: "a", sort_order: 10000 }), row({ id: "b", sort_order: 20000 })];
    const plan = planBoardMove(moveInput({ destRows, index: 1, sourceIndex: 0 }));
    expect(plan).toEqual({ patch: { sort_order: 15000 }, cycle: null, modules: null, reorderOnly: true });
  });

  it("applies the destination column value plus a fencepost sort", () => {
    const plan = planBoardMove(
      moveInput({ destColumn: DONE, destRows: [row({ id: "a", sort_order: 10000 })], index: 1 })
    );
    expect(plan).toEqual({
      patch: { state_id: DONE, sort_order: 10000 + SORT_ORDER_STEP },
      cycle: null,
      modules: null,
      reorderOnly: false,
    });
  });

  it("updates both dimensions across swimlanes", () => {
    const plan = planBoardMove(
      moveInput({
        subDimension: "labels",
        sourceLane: "None",
        destColumn: DONE,
        destLane: LABEL,
        destRows: [],
        index: 0,
      })
    );
    expect(plan).toEqual({
      patch: { state_id: DONE, label_ids: [LABEL], sort_order: SORT_ORDER_STEP },
      cycle: null,
      modules: null,
      reorderOnly: false,
    });
  });

  it("carries cycle and module bridges beside the sort patch", () => {
    const cyclePlan = planBoardMove(
      moveInput({ groupDimension: "cycle", sourceColumn: "None", destColumn: CYCLE, destRows: [], index: 0 })
    );
    expect(cyclePlan).toEqual({
      patch: { sort_order: SORT_ORDER_STEP },
      cycle: { add: CYCLE, remove: null },
      modules: null,
      reorderOnly: false,
    });
    const modulePlan = planBoardMove(
      moveInput({ groupDimension: "module", sourceColumn: "None", destColumn: MODULE, destRows: [], index: 0 })
    );
    expect(modulePlan).toEqual({
      patch: { sort_order: SORT_ORDER_STEP },
      cycle: null,
      modules: { add: [MODULE], remove: [] },
      reorderOnly: false,
    });
  });

  it("sends nothing for a cross-cell drop that changes no field", () => {
    expect(planBoardMove(moveInput({ groupDimension: "created_by", sourceColumn: "a", destColumn: "b" }))).toBeNull();
  });

  it("treats missing lanes as the flat column under a sub-group", () => {
    const plan = planBoardMove(moveInput({ subDimension: "labels", destColumn: DONE, destRows: [], index: 0 }));
    expect(plan).toEqual({
      patch: { state_id: DONE, sort_order: SORT_ORDER_STEP },
      cycle: null,
      modules: null,
      reorderOnly: false,
    });
  });
});

describe("insertionIndexForDrop", () => {
  const rows = [row({ id: "a" }), row({ id: "b" }), row({ id: "c" })];

  it("seats the card at the target edge without the moving card", () => {
    expect(insertionIndexForDrop(rows, "moving", 0, "b", "above")).toBe(1);
    expect(insertionIndexForDrop(rows, "moving", 0, "b", "below")).toBe(2);
  });

  it("drops bare column targets at the end", () => {
    expect(insertionIndexForDrop(rows, "moving", 0, null, null)).toBe(3);
  });

  it("keeps the card on its own seat while it hovers itself", () => {
    expect(insertionIndexForDrop(rows, "b", 1, "b", "below")).toBe(1);
  });

  it("drops unknown cards at the end", () => {
    expect(insertionIndexForDrop(rows, "moving", 0, "ghost", "above")).toBe(3);
  });
});

describe("boardDropRefusal", () => {
  const gate = {
    orderBy: "sort_order",
    groupDimension: "state" as string | null,
    subDimension: null as string | null,
    canEdit: true,
    destColumnKey: DONE,
    openCycleIds: new Set<string>(),
  };

  it("allows an editable manual drop", () => {
    expect(boardDropRefusal(gate)).toBeNull();
  });

  it("refuses without edit standing", () => {
    const refusal = boardDropRefusal({ ...gate, canEdit: false });
    expect(refusal?.overlay).toBeNull();
    expect(refusal?.toast).toMatch(/permission/);
  });

  it("refuses undraggable group and sub-group dimensions", () => {
    expect(boardDropRefusal({ ...gate, groupDimension: "created_by" })?.overlay).toBeNull();
    expect(boardDropRefusal({ ...gate, subDimension: "state_detail.group" })).not.toBeNull();
  });

  it("names the refusal title once", () => {
    expect(MOVE_REFUSED_TITLE).toBe("Cannot move the work item");
  });

  it("overlays the active ordering under a non-manual sort", () => {
    const refusal = boardDropRefusal({ ...gate, orderBy: "-priority" });
    expect(refusal?.overlay).toBe("Ordered by Priority");
    expect(refusal?.toast).toMatch(/ordered by Priority/);
  });

  it("refuses the priority value used by the NEWFRONT-154 repro", () => {
    const refusal = boardDropRefusal({ ...gate, orderBy: "priority" });
    expect(refusal?.overlay).toBe("Ordered by priority");
    expect(refusal?.toast).toBe(
      "Cards stay put while ordered by priority. Switch back to the manual order to move them."
    );
  });

  it("falls back to the raw order key without a label", () => {
    expect(boardDropRefusal({ ...gate, orderBy: "mystery" })?.overlay).toBe("Ordered by mystery");
  });

  it("refuses completed cycle columns but not open or empty ones", () => {
    const cycle = { ...gate, groupDimension: "cycle", openCycleIds: new Set([CYCLE]) };
    const refusal = boardDropRefusal({ ...cycle, destColumnKey: "past-cycle" });
    expect(refusal?.overlay).toMatch(/complet/i);
    expect(boardDropRefusal({ ...cycle, destColumnKey: CYCLE })).toBeNull();
    expect(boardDropRefusal({ ...cycle, destColumnKey: "None" })).toBeNull();
    expect(boardDropRefusal(gate)).toBeNull();
  });

  it("surfaces the edition workflow reason verbatim", () => {
    const refusal = boardDropRefusal({ ...gate, workflowBlockedReason: "Needs a review first" });
    expect(refusal).toEqual({ overlay: "Needs a review first", toast: "Needs a review first" });
  });
});
