// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board model (parity/specs/issues.md §5, ISS-028–035): column and lane
// shaping from the three envelope shapes, empty-group filtering, the
// quick-add prefill merge and the create gate.
import type { GroupedIssueListResponse, IssueListItem, SubGroupedIssueListResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  boardColumns,
  boardPrefill,
  boardQuickAddDisabledReason,
  effectiveSubGroup,
  flatBoard,
  groupedBoard,
  laneKeys,
  laneTotal,
  showsGroup,
  subGroupedBoard,
  subGroupedColumns,
} from "./model.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const LABEL = "423e4567-e89b-12d3-a456-426614174003";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
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

function grouped(results: Record<string, { rows: IssueListItem[]; total: number }>): GroupedIssueListResponse {
  const buckets: GroupedIssueListResponse["results"] = {};
  for (const [key, bucket] of Object.entries(results)) {
    buckets[key] = { results: bucket.rows as never, total_results: bucket.total };
  }
  return {
    grouped_by: "state_id",
    sub_grouped_by: null,
    total_count: Object.values(results).reduce((sum, bucket) => sum + bucket.total, 0),
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: Object.values(results).reduce((sum, bucket) => sum + bucket.rows.length, 0),
    total_results: Object.values(results).reduce((sum, bucket) => sum + bucket.total, 0),
    results: buckets,
  };
}

function subGrouped(
  results: Record<string, Record<string, { rows: IssueListItem[]; total: number }>>
): SubGroupedIssueListResponse {
  const groups: SubGroupedIssueListResponse["results"] = {};
  let total = 0;
  for (const [groupKey, subBuckets] of Object.entries(results)) {
    const nested: Record<string, { results: never; total_results: number }> = {};
    let groupTotal = 0;
    for (const [subKey, bucket] of Object.entries(subBuckets)) {
      nested[subKey] = { results: bucket.rows as never, total_results: bucket.total };
      groupTotal += bucket.total;
    }
    groups[groupKey] = { results: nested, total_results: groupTotal };
    total += groupTotal;
  }
  return {
    grouped_by: "state_id",
    sub_grouped_by: "labels__id",
    total_count: total,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: total,
    total_results: total,
    results: groups,
  };
}

const labels = new Map([
  [LABEL, "Bug"],
  [STATE, "Todo"],
]);

describe("boardColumns", () => {
  it("keeps the server order with the empty bucket last and names every bucket", () => {
    const columns = boardColumns(
      grouped({
        None: { rows: [], total: 0 },
        [LABEL]: { rows: [], total: 2 },
        [STATE]: { rows: [row()], total: 3 },
      }),
      labels,
      { dimension: "state", showEmpty: true }
    );
    expect(columns).toEqual([
      { key: LABEL, title: "Bug", total: 2 },
      { key: STATE, title: "Todo", total: 3 },
      { key: "None", title: "None", total: 0 },
    ]);
  });

  it("drops zero-count columns when the empty-group flag is off", () => {
    const columns = boardColumns(
      grouped({ [LABEL]: { rows: [], total: 0 }, None: { rows: [row()], total: 1 } }),
      labels,
      {
        dimension: "labels",
        showEmpty: false,
      }
    );
    expect(columns.map((column) => column.key)).toEqual(["None"]);
  });

  it("skips a bucket key with no bucket behind it", () => {
    const response = grouped({ [LABEL]: { rows: [], total: 1 } });
    Object.defineProperty(response.results, "ghost", { value: undefined, enumerable: true });
    const columns = boardColumns(response, labels, { dimension: "labels", showEmpty: true });
    expect(columns.map((column) => column.key)).toEqual([LABEL]);
  });

  it("falls back to the dimension title when a bucket has no name", () => {
    const columns = boardColumns(grouped({ high: { rows: [], total: 1 } }), new Map(), {
      dimension: "priority",
      showEmpty: true,
    });
    expect(columns[0]?.title).toBe("High");
  });
});

describe("flatBoard", () => {
  it("answers one column titled like the list's flat section", () => {
    const board = flatBoard({ results: [row()], total_count: 4 });
    expect(board.columns).toHaveLength(1);
    expect(board.columns[0]?.key).toBe("all");
    expect(board.lanes).toHaveLength(1);
    expect(board.lanes[0]?.cells["all"]?.rows).toHaveLength(1);
    expect(board.subGrouped).toBe(false);
    expect(board.total).toBe(4);
  });
});

describe("groupedBoard", () => {
  it("normalizes the m2m join key back into the row's arrays", () => {
    const bucket = grouped({ [LABEL]: { rows: [row({ label_ids: undefined as never })], total: 1 } });
    bucket.results[LABEL]!.results[0] = {
      ...row(),
      label_ids: undefined as never,
      labels__id: LABEL,
    } as never;
    const board = groupedBoard(bucket, { dimension: "labels", labels, showEmpty: true });
    expect(board.lanes[0]?.cells[LABEL]?.rows[0]?.label_ids).toEqual([LABEL]);
  });

  it("omits the cells of columns the empty-group flag hides", () => {
    const board = groupedBoard(grouped({ [STATE]: { rows: [], total: 0 }, [LABEL]: { rows: [row()], total: 1 } }), {
      dimension: "labels",
      labels,
      showEmpty: false,
    });
    expect(Object.keys(board.lanes[0]?.cells ?? {})).toEqual([LABEL]);
  });
});

describe("sub-grouped lanes", () => {
  const response = subGrouped({
    [STATE]: {
      [LABEL]: { rows: [row()], total: 1 },
      None: { rows: [row({ id: "b", label_ids: [] })], total: 2 },
    },
    "state-2": {
      None: { rows: [row({ id: "c" })], total: 1 },
    },
  });

  it("orders lanes by first appearance with None last and sums across columns", () => {
    expect(laneKeys(response, true)).toEqual([LABEL, "None"]);
    expect(laneTotal(response, "None")).toBe(3);
    expect(laneTotal(response, LABEL)).toBe(1);
  });

  it("hides lanes with no issues when show-empty is off", () => {
    expect(laneKeys(response, false)).toEqual([LABEL, "None"]);
    expect(laneKeys(subGrouped({ [STATE]: { [LABEL]: { rows: [], total: 0 } } }), false)).toEqual([]);
  });

  it("builds columns with server group totals and lanes with their cells", () => {
    const board = subGroupedBoard(response, {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    expect(board.columns.map((column) => [column.key, column.total])).toEqual([
      [STATE, 3],
      ["state-2", 1],
    ]);
    expect(board.subGrouped).toBe(true);
    expect(board.lanes.map((lane) => [lane.key, lane.title, lane.total])).toEqual([
      [LABEL, "Bug", 1],
      ["None", "None", 3],
    ]);
    expect(board.lanes[1]?.cells[STATE]?.rows).toHaveLength(1);
    expect(board.lanes[0]?.cells["state-2"]?.total).toBe(0);
  });

  it("skips a sub-grouped bucket key with no bucket behind it", () => {
    const response = subGrouped({ [STATE]: { [LABEL]: { rows: [], total: 1 } } });
    Object.defineProperty(response.results, "ghost", { value: undefined, enumerable: true });
    const columns = subGroupedColumns(response, labels, { dimension: "state", showEmpty: true });
    expect(columns.map((column) => column.key)).toEqual([STATE]);
  });

  it("drops a zero-count column and its cells", () => {
    const columns = subGroupedColumns(subGrouped({ [STATE]: { [LABEL]: { rows: [], total: 0 } } }), labels, {
      dimension: "state",
      showEmpty: false,
    });
    expect(columns).toEqual([]);
  });
});

describe("effectiveSubGroup", () => {
  it("ignores a sub-group that repeats the group or has no group", () => {
    expect(effectiveSubGroup("state", "state")).toBeNull();
    expect(effectiveSubGroup(null, "priority")).toBeNull();
    expect(effectiveSubGroup("state", null)).toBeNull();
    expect(effectiveSubGroup("state", "priority")).toBe("priority");
  });
});

describe("boardPrefill", () => {
  it("merges the column value with the lane value", () => {
    expect(boardPrefill("state", STATE, "labels", LABEL)).toEqual({
      state_id: STATE,
      label_ids: [LABEL],
    });
  });

  it("clears nothing for the empty buckets and keeps list fields", () => {
    expect(boardPrefill("labels", "None", "assignees", "None")).toEqual({});
    expect(boardPrefill("assignees", "member-1", null, null)).toEqual({ assignee_ids: ["member-1"] });
    expect(boardPrefill("cycle", "cycle-1", "module", "module-1")).toEqual({
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
    });
  });

  it("returns no fields for an ungrouped or unsupported dimension", () => {
    expect(boardPrefill(null, "all", null, null)).toEqual({});
    expect(boardPrefill("team_project", "project-1", null, null)).toEqual({});
  });
});

describe("boardQuickAddDisabledReason", () => {
  it("offers quick-add in every grouping for an editor", () => {
    expect(boardQuickAddDisabledReason({ canEdit: true })).toBeUndefined();
  });

  it("reports permission, completed-cycle and workflow reasons in order", () => {
    expect(boardQuickAddDisabledReason({ canEdit: false, completedCycle: true })).toBe(
      "New work items are read-only for you"
    );
    expect(boardQuickAddDisabledReason({ canEdit: true, completedCycle: true })).toBe("This cycle is complete");
    expect(boardQuickAddDisabledReason({ canEdit: true, workflowBlockedReason: "Blocked by a workflow rule" })).toBe(
      "Blocked by a workflow rule"
    );
  });
});

describe("showsGroup", () => {
  it("keeps empty groups only while show-empty is on", () => {
    expect(showsGroup(0, true)).toBe(true);
    expect(showsGroup(0, false)).toBe(false);
    expect(showsGroup(2, false)).toBe(true);
  });
});
