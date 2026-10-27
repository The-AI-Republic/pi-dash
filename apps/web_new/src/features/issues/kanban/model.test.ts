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
  boardCreateInitialValues,
  boardHasMore,
  boardPageCursor,
  boardPrefill,
  boardQuickAddDisabledReason,
  bucketTotal,
  effectiveSubGroup,
  flatBoard,
  groupedBoard,
  laneKeys,
  laneTotal,
  mergeBoardPage,
  showsGroup,
  subGroupedBoard,
  subGroupedColumns,
  type BoardData,
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

  it("counts the rendered rows when the server total undercounts (NEWFRONT-153)", () => {
    const columns = boardColumns(
      grouped({ [STATE]: { rows: [row({ id: "a" }), row({ id: "b" }), row({ id: "c" })], total: 2 } }),
      labels,
      { dimension: "state", showEmpty: true }
    );
    expect(columns).toEqual([{ key: STATE, title: "Todo", total: 3 }]);
  });

  it("keeps a column whose rows render despite a zero server total when show-empty is off", () => {
    const columns = boardColumns(grouped({ [STATE]: { rows: [row()], total: 0 } }), labels, {
      dimension: "state",
      showEmpty: false,
    });
    expect(columns.map((column) => column.key)).toEqual([STATE]);
  });
});

describe("bucketTotal", () => {
  it("takes the larger of the server total and the rendered rows", () => {
    expect(bucketTotal({ results: [row(), row(), row()] as never, total_results: 2 })).toBe(3);
    expect(bucketTotal({ results: [row()] as never, total_results: 4 })).toBe(4);
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

  it("reconciles the column and cell totals with the rendered rows (NEWFRONT-153)", () => {
    const board = groupedBoard(
      grouped({ [STATE]: { rows: [row({ id: "a" }), row({ id: "b" }), row({ id: "c" })], total: 2 } }),
      { dimension: "state", labels, showEmpty: true }
    );
    expect(board.columns[0]?.total).toBe(3);
    expect(board.lanes[0]?.cells[STATE]?.total).toBe(3);
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

  it("counts rendered rows when a sub-bucket total undercounts (NEWFRONT-153)", () => {
    const under = subGrouped({
      [STATE]: {
        [LABEL]: { rows: [row({ id: "a" }), row({ id: "b" })], total: 1 },
        None: { rows: [row({ id: "c" })], total: 1 },
      },
    });
    expect(laneTotal(under, LABEL)).toBe(2);
    expect(laneTotal(under, "None")).toBe(1);
    const columns = subGroupedColumns(under, labels, { dimension: "state", showEmpty: true });
    expect(columns).toEqual([{ key: STATE, title: "Todo", total: 3 }]);
    const board = subGroupedBoard(under, {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    expect(board.lanes.map((lane) => [lane.key, lane.total])).toEqual([
      [LABEL, 2],
      ["None", 1],
    ]);
    expect(board.lanes[0]?.cells[STATE]?.total).toBe(2);
  });

  it("keeps a larger server group total over the sub-bucket rows", () => {
    const response = subGrouped({ [STATE]: { [LABEL]: { rows: [row()], total: 1 } } });
    response.results[STATE]!.total_results = 9;
    const columns = subGroupedColumns(response, labels, { dimension: "state", showEmpty: true });
    expect(columns[0]?.total).toBe(9);
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

describe("boardHasMore and boardPageCursor", () => {
  it("compares loaded rows against the total", () => {
    expect(boardHasMore(1, 4)).toBe(true);
    expect(boardHasMore(4, 4)).toBe(false);
  });

  it("asks for the page starting at the loaded count", () => {
    expect(boardPageCursor(0, 30)).toBe("30:0:0");
    expect(boardPageCursor(30, 30)).toBe("30:1:0");
    expect(boardPageCursor(10, 10)).toBe("10:1:0");
  });
});

function boardData(overrides: Partial<BoardData> = {}): BoardData {
  return {
    columns: [{ key: STATE, title: "Todo", total: 4 }],
    lanes: [
      {
        key: "",
        title: "",
        total: 4,
        cells: { [STATE]: { columnKey: STATE, rows: [row()], total: 4 } },
      },
    ],
    subGrouped: false,
    total: 4,
    ...overrides,
  };
}

describe("mergeBoardPage", () => {
  it("appends later rows per cell without duplicates and refreshes totals", () => {
    const current = boardData();
    const incoming = boardData({
      columns: [{ key: STATE, title: "Todo", total: 5 }],
      lanes: [
        {
          key: "",
          title: "",
          total: 5,
          cells: {
            [STATE]: {
              columnKey: STATE,
              rows: [row(), row({ id: "next", name: "Ship it" })],
              total: 5,
            },
          },
        },
      ],
      total: 5,
    });
    const merged = mergeBoardPage(current, incoming);
    expect(merged.columns[0]?.total).toBe(5);
    expect(merged.lanes[0]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual([
      "123e4567-e89b-12d3-a456-426614174000",
      "next",
    ]);
    expect(merged.lanes[0]?.cells[STATE]?.total).toBe(5);
    expect(merged.total).toBe(5);
  });

  it("joins cells, lanes and columns the first page never saw", () => {
    const current = boardData();
    const incoming = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 4 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 4,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row()], total: 4 },
            done: { columnKey: "done", rows: [row({ id: "d" })], total: 1 },
          },
        },
        {
          key: "bug",
          title: "Bug",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "b" })], total: 1 } },
        },
      ],
      total: 6,
    });
    const merged = mergeBoardPage(current, incoming);
    expect(merged.columns.map((column) => column.key)).toEqual([STATE, "done"]);
    expect(merged.lanes.map((lane) => lane.key)).toEqual(["", "bug"]);
    expect(merged.lanes[0]?.cells["done"]?.rows).toHaveLength(1);
  });

  it("keeps columns and lanes the later page drops", () => {
    const current = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 4 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 4,
          cells: { [STATE]: { columnKey: STATE, rows: [row()], total: 4 } },
        },
        { key: "bug", title: "Bug", total: 1, cells: {} },
      ],
    });
    const merged = mergeBoardPage(current, boardData());
    expect(merged.columns.map((column) => column.key)).toEqual([STATE, "done"]);
    expect(merged.lanes.map((lane) => lane.key)).toEqual(["", "bug"]);
  });

  it("merges a later grouped page into the requesting column only", () => {
    const current = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 4 },
        { key: "done", title: "Done", total: 3 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 7,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row(), row({ id: "a2" })], total: 4 },
            done: { columnKey: "done", rows: [row({ id: "b1" })], total: 3 },
          },
        },
      ],
      total: 7,
    });
    const incoming = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 4 },
        { key: "done", title: "Done", total: 5 },
        { key: "fresh", title: "Fresh", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 10,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row({ id: "a3" })], total: 4 },
            done: { columnKey: "done", rows: [row({ id: "wrong-window" })], total: 5 },
            fresh: { columnKey: "fresh", rows: [row({ id: "c1" })], total: 1 },
          },
        },
        {
          key: "bug",
          title: "Bug",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "stray" })], total: 1 } },
        },
      ],
      total: 11,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "column", columnKey: STATE });
    expect(merged.lanes[0]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual([
      "123e4567-e89b-12d3-a456-426614174000",
      "a2",
      "a3",
    ]);
    expect(merged.lanes[0]?.cells["done"]?.rows.map((entry) => entry.id)).toEqual(["b1"]);
    expect(merged.lanes[0]?.cells["done"]?.total).toBe(5);
    expect(merged.lanes[0]?.cells["fresh"]).toBeUndefined();
    expect(merged.columns.map((column) => column.key)).toEqual([STATE, "done"]);
    expect(merged.lanes.map((lane) => lane.key)).toEqual([""]);
    expect(merged.total).toBe(11);
  });

  it("merges a later cell page into the requesting cell only", () => {
    const current = boardData({
      subGrouped: true,
      columns: [{ key: STATE, title: "Todo", total: 2 }],
      lanes: [
        {
          key: "bug",
          title: "Bug",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row()], total: 1 } },
        },
        {
          key: "None",
          title: "None",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "n1" })], total: 1 } },
        },
      ],
      total: 2,
    });
    const incoming = boardData({
      subGrouped: true,
      columns: [{ key: STATE, title: "Todo", total: 3 }],
      lanes: [
        {
          key: "bug",
          title: "",
          total: 2,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "b2" })], total: 2 } },
        },
        {
          key: "None",
          title: "",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "wrong-window" })], total: 1 } },
        },
        {
          key: "fresh",
          title: "Fresh",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "f1" })], total: 1 } },
        },
      ],
      total: 4,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "cell", columnKey: STATE, laneKey: "bug" });
    expect(merged.lanes[0]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual([
      "123e4567-e89b-12d3-a456-426614174000",
      "b2",
    ]);
    expect(merged.lanes[1]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual(["n1"]);
    expect(merged.lanes.map((lane) => lane.key)).toEqual(["bug", "None"]);
  });

  it("joins a newly discovered owner lane with only the owner cell", () => {
    const current = boardData({
      subGrouped: true,
      columns: [{ key: STATE, title: "Todo", total: 1 }],
      lanes: [
        {
          key: "None",
          title: "None",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "n1" })], total: 1 } },
        },
      ],
      total: 1,
    });
    const incoming = boardData({
      subGrouped: true,
      columns: [
        { key: STATE, title: "Todo", total: 2 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "None",
          title: "",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "wrong-window" })], total: 1 } },
        },
        {
          key: "bug",
          title: "Bug",
          total: 2,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row({ id: "b1" })], total: 1 },
            done: { columnKey: "done", rows: [row({ id: "d1" })], total: 1 },
          },
        },
      ],
      total: 3,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "cell", columnKey: STATE, laneKey: "bug" });
    const bug = merged.lanes.find((lane) => lane.key === "bug");
    expect(Object.keys(bug?.cells ?? {})).toEqual([STATE]);
    expect(bug?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual(["b1"]);
    expect(merged.columns.map((column) => column.key)).toEqual([STATE]);
    expect(merged.lanes[0]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual(["n1"]);
  });

  it("keeps column pages out of named lanes", () => {
    const current = boardData({
      subGrouped: true,
      columns: [{ key: STATE, title: "Todo", total: 1 }],
      lanes: [
        {
          key: "bug",
          title: "Bug",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row()], total: 1 } },
        },
      ],
      total: 1,
    });
    const incoming = boardData({
      subGrouped: true,
      columns: [{ key: STATE, title: "Todo", total: 2 }],
      lanes: [
        {
          key: "bug",
          title: "",
          total: 2,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "b2" })], total: 2 } },
        },
      ],
      total: 2,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "column", columnKey: STATE });
    expect(merged.lanes[0]?.cells[STATE]?.rows).toHaveLength(1);
    expect(merged.lanes[0]?.cells[STATE]?.total).toBe(2);
  });

  it("joins a newly discovered owner column with its cell", () => {
    const current = boardData({
      columns: [{ key: STATE, title: "Todo", total: 1 }],
      lanes: [
        {
          key: "",
          title: "",
          total: 1,
          cells: { [STATE]: { columnKey: STATE, rows: [row()], total: 1 } },
        },
      ],
      total: 1,
    });
    const incoming = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 1 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 2,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row({ id: "wrong-window" })], total: 1 },
            done: { columnKey: "done", rows: [row({ id: "d1" })], total: 1 },
          },
        },
      ],
      total: 2,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "column", columnKey: "done" });
    expect(merged.columns.map((column) => column.key)).toEqual([STATE, "done"]);
    expect(merged.lanes[0]?.cells["done"]?.rows.map((entry) => entry.id)).toEqual(["d1"]);
    expect(merged.lanes[0]?.cells[STATE]?.rows).toHaveLength(1);
  });

  it("keeps merged totals at or above the rendered rows (NEWFRONT-153)", () => {
    const current = boardData({
      columns: [{ key: STATE, title: "Todo", total: 2 }],
      lanes: [
        {
          key: "",
          title: "",
          total: 2,
          cells: {
            [STATE]: {
              columnKey: STATE,
              rows: [row({ id: "a" }), row({ id: "b" }), row({ id: "c" })],
              total: 2,
            },
          },
        },
      ],
      total: 2,
    });
    const incoming = boardData({
      columns: [{ key: STATE, title: "Todo", total: 2 }],
      lanes: [
        {
          key: "",
          title: "",
          total: 2,
          cells: { [STATE]: { columnKey: STATE, rows: [row({ id: "a" })], total: 2 } },
        },
      ],
      total: 2,
    });
    const merged = mergeBoardPage(current, incoming);
    expect(merged.lanes[0]?.cells[STATE]?.rows).toHaveLength(3);
    expect(merged.lanes[0]?.cells[STATE]?.total).toBe(3);
    expect(merged.columns[0]?.total).toBe(3);
    expect(merged.lanes[0]?.total).toBe(3);
  });

  it("never drops a non-owner total below its rendered rows (NEWFRONT-153)", () => {
    const current = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 2 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 3,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row({ id: "a" })], total: 2 },
            done: { columnKey: "done", rows: [row({ id: "b1" }), row({ id: "b2" })], total: 1 },
          },
        },
      ],
      total: 3,
    });
    const incoming = boardData({
      columns: [
        { key: STATE, title: "Todo", total: 2 },
        { key: "done", title: "Done", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 3,
          cells: {
            [STATE]: { columnKey: STATE, rows: [row({ id: "a3" })], total: 2 },
            done: { columnKey: "done", rows: [row({ id: "b1" })], total: 1 },
          },
        },
      ],
      total: 3,
    });
    const merged = mergeBoardPage(current, incoming, { kind: "column", columnKey: STATE });
    expect(merged.lanes[0]?.cells["done"]?.rows.map((entry) => entry.id)).toEqual(["b1", "b2"]);
    expect(merged.lanes[0]?.cells["done"]?.total).toBe(2);
    expect(merged.columns.find((column) => column.key === "done")?.total).toBe(2);
  });
});

describe("boardCreateInitialValues", () => {
  it("translates the cell prefill to the dialog's values", () => {
    expect(
      boardCreateInitialValues({
        state_id: STATE,
        priority: "high",
        assignee_ids: ["member"],
        label_ids: [LABEL],
        cycle_id: "cycle",
        module_ids: ["module"],
      })
    ).toEqual({
      stateId: STATE,
      priority: "high",
      assigneeIds: ["member"],
      labelIds: [LABEL],
      cycleId: "cycle",
      moduleIds: ["module"],
    });
  });

  it("leaves the form blank without a prefill", () => {
    expect(boardCreateInitialValues({})).toEqual({});
  });
});
