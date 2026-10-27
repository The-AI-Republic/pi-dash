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
  reconcileLabelGhosts,
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
    const columns = boardColumns(response, labels, { dimension: "labels", showEmpty: false });
    expect(columns.map((column) => column.key)).toEqual([LABEL]);
  });

  it("adds an unused project label as a zero-total column with empty groups on", () => {
    const columns = boardColumns(grouped({ None: { rows: [row(), row(), row()], total: 3 } }), labels, {
      dimension: "labels",
      showEmpty: true,
      domain: [LABEL],
    });
    expect(columns).toEqual([
      { key: LABEL, title: "Bug", total: 0 },
      { key: "None", title: "None", total: 3 },
    ]);
  });

  it("hides the unused label again when empty groups are off", () => {
    const columns = boardColumns(grouped({ None: { rows: [row()], total: 1 } }), labels, {
      dimension: "labels",
      showEmpty: false,
      domain: [LABEL],
    });
    expect(columns.map((column) => column.key)).toEqual(["None"]);
  });

  it("adds an unused project module and keeps its cell empty", () => {
    const module = "523e4567-e89b-12d3-a456-426614174004";
    const board = groupedBoard(grouped({ None: { rows: [row()], total: 1 } }), {
      dimension: "module",
      labels: new Map([[module, "Payments"]]),
      showEmpty: true,
      domain: [module],
    });
    expect(board.columns).toEqual([
      { key: module, title: "Payments", total: 0 },
      { key: "None", title: "None", total: 1 },
    ]);
    expect(board.lanes[0]?.cells[module]).toEqual({ columnKey: module, rows: [], total: 0 });
  });

  it("synthesizes the empty cycle column when the board has only cycles", () => {
    const cycle = "623e4567-e89b-12d3-a456-426614174005";
    const board = groupedBoard(grouped({ [cycle]: { rows: [row()], total: 1 } }), {
      dimension: "cycle",
      labels: new Map([[cycle, "Sprint 8"]]),
      showEmpty: true,
      domain: [cycle],
    });
    expect(board.columns.map((column) => column.key)).toEqual([cycle, "None"]);
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

  it("adds an unused project label as a zero-total lane with empty groups on", () => {
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row()], total: 3 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
      subDomain: [LABEL],
    });
    expect(board.lanes.map((lane) => [lane.key, lane.title, lane.total])).toEqual([
      [LABEL, "Bug", 0],
      ["None", "None", 3],
    ]);
    expect(board.lanes[0]?.cells[STATE]).toEqual({ columnKey: STATE, rows: [], total: 0 });
  });

  it("drops the unused lane again when empty groups are off", () => {
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row()], total: 3 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: false,
      subDomain: [LABEL],
    });
    expect(board.lanes.map((lane) => lane.key)).toEqual(["None"]);
  });

  it("treats a label already in the envelope as one lane", () => {
    const response = subGrouped({ [STATE]: { [LABEL]: { rows: [row()], total: 1 } } });
    expect(laneKeys(response, true, { dimension: "labels", domain: [LABEL] })).toEqual([LABEL, "None"]);
    expect(laneKeys(response, true, { dimension: "state", domain: [LABEL] })).toEqual([LABEL]);
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

describe("label ghost reconciliation (ISS-031/NEWFRONT-158)", () => {
  it("returns the ghosted row to None with counts that match the rendered cards", () => {
    const ghost = row({ id: "ghost", label_ids: [] });
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    // The real envelope: the server drops the ghost from its lanes and from
    // total_count alike, so the lanes already sum to the total.
    const board = subGroupedBoard(
      subGrouped({
        [STATE]: {
          [LABEL]: { rows: [kept], total: 1 },
          None: { rows: [plain], total: 1 },
        },
      }),
      {
        groupDimension: "state",
        subDimension: "labels",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    expect(board.total).toBe(2);
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [kept, ghost, plain], total: 3 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    const result = reconcileLabelGhosts(board, fallback, "sub");
    expect(result.lanes.map((lane) => [lane.key, lane.total])).toEqual([
      [LABEL, 1],
      ["None", 2],
    ]);
    expect(result.lanes[1]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual(["plain", "ghost"]);
    expect(result.lanes[1]?.cells[STATE]?.total).toBe(2);
    expect(result.columns[0]?.total).toBe(3);
    expect(result.total).toBe(3);
    // Every count equals the cards behind it.
    const cards = result.lanes.reduce((sum, lane) => sum + (lane.cells[STATE]?.rows.length ?? 0), 0);
    expect(result.columns[0]?.total).toBe(cards);
    expect(result.lanes.reduce((sum, lane) => sum + lane.total, 0)).toBe(cards);
  });

  it("keeps a live-labeled row in its label lane and a never-labeled row under None", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    const board = subGroupedBoard(
      subGrouped({
        [STATE]: {
          [LABEL]: { rows: [kept], total: 1 },
          None: { rows: [plain], total: 1 },
        },
      }),
      {
        groupDimension: "state",
        subDimension: "labels",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [kept, plain], total: 2 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "sub")).toBe(board);
  });

  it("creates the None lane when the server omitted it entirely", () => {
    const second = "623e4567-e89b-12d3-a456-426614174005";
    const kept = row({ label_ids: [LABEL] });
    const board = subGroupedBoard(
      subGrouped({
        [STATE]: { [LABEL]: { rows: [kept], total: 1 } },
        [second]: { [LABEL]: { rows: [row({ id: "kept-2", state_id: second, label_ids: [LABEL] })], total: 1 } },
      }),
      {
        groupDimension: "state",
        subDimension: "labels",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const fallback = groupedBoard(
      grouped({
        [STATE]: { rows: [kept, row({ id: "ghost" })], total: 2 },
        [second]: { rows: [row({ id: "kept-2", state_id: second, label_ids: [LABEL] })], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "sub");
    expect(result.lanes.map((lane) => [lane.key, lane.total])).toEqual([
      [LABEL, 2],
      ["None", 1],
    ]);
    expect(result.columns.map((column) => column.total)).toEqual([2, 1]);
    // The empty lane already existed (labels always draw their None lane
    // with empty groups on); the recovered row fills only the first cell.
    expect(result.lanes[1]?.cells[second]).toEqual({ columnKey: second, rows: [], total: 0 });
  });

  it("adds a column the labels read dropped entirely so its ghost can render", () => {
    const second = "623e4567-e89b-12d3-a456-426614174005";
    const ghost = row({ id: "ghost", state_id: second });
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row({ id: "plain" })], total: 1 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    const fallback = groupedBoard(
      grouped({
        [STATE]: { rows: [row({ id: "plain" })], total: 1 },
        [second]: { rows: [ghost], total: 1 },
      }),
      { dimension: "state", labels: new Map([...labels, [second, "Done"]]), showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "sub");
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([
      [STATE, 1],
      [second, 1],
    ]);
    expect(result.lanes.map((lane) => [lane.key, lane.total])).toEqual([["None", 2]]);
    expect(result.lanes[0]?.cells[second]?.rows.map((entry) => entry.id)).toEqual(["ghost"]);
    expect(result.total).toBe(2);
  });

  it("does not add a fallback-only column that recovered nothing", () => {
    const second = "623e4567-e89b-12d3-a456-426614174005";
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row({ id: "plain" })], total: 1 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    const fallback = groupedBoard(
      grouped({
        [STATE]: { rows: [row({ id: "plain" }), row({ id: "ghost" })], total: 2 },
        [second]: { rows: [row({ id: "kept-2", state_id: second, label_ids: [LABEL] })], total: 1 },
      }),
      { dimension: "state", labels: new Map([...labels, [second, "Done"]]), showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "sub");
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([[STATE, 2]]);
    expect(result.lanes.map((lane) => [lane.key, lane.total])).toEqual([["None", 2]]);
    expect(result.total).toBe(2);
  });

  it("waits for every lane's column window before recovering", () => {
    const ghost = row({ id: "ghost" });
    const plain = row({ id: "plain" });
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [plain], total: 40 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [plain, ghost], total: 2 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "sub")).toBe(board);
  });

  it("ignores a fallback whose column is only a partial page", () => {
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row({ id: "plain" })], total: 1 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [row({ id: "ghost" })], total: 50 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "sub")).toBe(board);
  });

  it("leaves the board alone without a fallback lane or a missing row", () => {
    const board = subGroupedBoard(subGrouped({ [STATE]: { None: { rows: [row()], total: 1 } } }), {
      groupDimension: "state",
      subDimension: "labels",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, { ...board, subGrouped: false, lanes: [] }, "sub")).toBe(board);
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [row()], total: 1 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "sub")).toBe(board);
  });

  it("adds a ghost into a None lane with no cell yet and updates only that column", () => {
    const second = "623e4567-e89b-12d3-a456-426614174005";
    const ghost = row({ id: "ghost" });
    const board = subGroupedBoard(
      subGrouped({
        [STATE]: {
          [LABEL]: { rows: [row({ label_ids: [LABEL] })], total: 1 },
          None: { rows: [], total: 0 },
        },
        [second]: {
          [LABEL]: { rows: [row({ id: "kept-2", state_id: second, label_ids: [LABEL] })], total: 1 },
          None: { rows: [], total: 0 },
        },
      }),
      {
        groupDimension: "state",
        subDimension: "labels",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const noneLane = board.lanes[1]!;
    delete noneLane.cells[STATE];
    const fallback = groupedBoard(
      grouped({
        [STATE]: { rows: [ghost], total: 1 },
        [second]: { rows: [row({ id: "kept-2", state_id: second, label_ids: [LABEL] })], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "sub");
    expect(result.columns.map((column) => column.total)).toEqual([1, 1]);
    expect(result.lanes[1]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual(["ghost"]);
    expect(result.lanes[1]?.cells[STATE]?.total).toBe(1);
    expect(result.lanes[1]?.cells[second]?.total).toBe(0);
  });

  it("does not reconcile a non-sub-grouped board", () => {
    const fallback = groupedBoard(grouped({ [STATE]: { rows: [row({ id: "ghost" })], total: 1 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(fallback, fallback, "sub")).toBe(fallback);
  });
});

describe("label ghost reconciliation on the main axis (ISS-028/NEWFRONT-961)", () => {
  const flatEnvelope = {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: [],
  };

  /** A labels-grouped board with its None column and the rows it draws. */
  function labelsBoard(results: Record<string, { rows: IssueListItem[]; total: number }>, showEmpty = true): BoardData {
    return groupedBoard(grouped(results), { dimension: "labels", labels, showEmpty });
  }

  it("returns a deleted-label row to the None column with matching counts", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    const ghost = row({ id: "ghost", label_ids: [] });
    // The real envelope: the server drops the ghost from the None column
    // and from the totals alike, so the columns already sum to the total.
    const board = labelsBoard({
      [LABEL]: { rows: [kept], total: 1 },
      None: { rows: [plain], total: 1 },
    });
    expect(board.total).toBe(2);
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [kept, ghost, plain],
      total_count: 3,
    });
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([
      [LABEL, 1],
      ["None", 2],
    ]);
    expect(result.lanes[0]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["plain", "ghost"]);
    expect(result.lanes[0]?.cells["None"]?.total).toBe(2);
    expect(result.lanes[0]?.cells[LABEL]?.rows.map((entry) => entry.id)).toEqual(["kept"]);
    expect(result.total).toBe(3);
    // Every header count equals the cards behind it.
    const cards =
      (result.lanes[0]?.cells[LABEL]?.rows.length ?? 0) + (result.lanes[0]?.cells["None"]?.rows.length ?? 0);
    expect(result.columns.reduce((sum, column) => sum + column.total, 0)).toBe(cards);
  });

  it("keeps the aggregate count when the ghost is only partly in the drawn window", () => {
    const plain = row({ id: "plain", label_ids: [] });
    const ghost = row({ id: "ghost", label_ids: [] });
    const board = labelsBoard({ None: { rows: [plain], total: 1 } });
    // The None column's aggregate says two unlabeled rows; the flat read
    // only carries one more card than the board shows.
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [plain, ghost],
      total_count: 2,
    });
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.columns.map((column) => column.total)).toEqual([2]);
    expect(result.lanes[0]?.cells["None"]?.total).toBe(2);
    expect(result.lanes[0]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["plain", "ghost"]);
  });

  it("adds the None column when the labels read dropped it whole", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const ghost = row({ id: "ghost", label_ids: [] });
    // With empty groups hidden the envelope's keys stay the whole domain
    // (NEWFRONT-965), so a labels read with no None bucket yields no None
    // column for recovery to fill. Recovery itself runs in both modes.
    const board = labelsBoard({ [LABEL]: { rows: [kept], total: 1 } }, false);
    expect(board.columns.map((column) => column.key)).toEqual([LABEL]);
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [kept, ghost],
      total_count: 2,
    });
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.columns.map((column) => [column.key, column.title, column.total])).toEqual([
      [LABEL, "Bug", 1],
      ["None", "None", 1],
    ]);
    expect(result.lanes[0]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["ghost"]);
    expect(result.total).toBe(2);
  });

  it("recovers a ghost into the synthetic empty None column", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const ghost = row({ id: "ghost", label_ids: [] });
    // With empty groups visible the builder draws the synthetic None
    // column at zero total (NEWFRONT-965); recovery fills that column
    // rather than appending a second one.
    const board = labelsBoard({ [LABEL]: { rows: [kept], total: 1 } });
    expect(board.columns.map((column) => [column.key, column.total])).toEqual([
      [LABEL, 1],
      ["None", 0],
    ]);
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [kept, ghost],
      total_count: 2,
    });
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.columns.map((column) => [column.key, column.title, column.total])).toEqual([
      [LABEL, "Bug", 1],
      ["None", "None", 1],
    ]);
    expect(result.lanes[0]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["ghost"]);
    expect(result.total).toBe(2);
  });

  it("does not duplicate a row the None column already renders", () => {
    const plain = row({ id: "plain", label_ids: [] });
    const board = labelsBoard({ None: { rows: [plain], total: 1 } });
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [plain],
      total_count: 1,
    });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
  });

  it("leaves a ghost-free labels board untouched", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    const board = labelsBoard({
      [LABEL]: { rows: [kept], total: 1 },
      None: { rows: [plain], total: 1 },
    });
    const fallback = flatBoard({
      ...flatEnvelope,
      results: [kept, plain],
      total_count: 2,
    });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
  });

  it("leaves a ghost-free labels board with a paged None column untouched", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    // The None column holds two of its forty rows: the rest are simply not
    // paged in yet, and its own paging must keep the column.
    const board = labelsBoard({
      [LABEL]: { rows: [kept], total: 1 },
      None: { rows: [plain], total: 40 },
    });
    const fallback = flatBoard({ ...flatEnvelope, results: [kept, plain], total_count: 2 });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
  });

  it("waits for the None window to close before recovering a ghost", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const plain = row({ id: "plain", label_ids: [] });
    const ghost = row({ id: "ghost", label_ids: [] });
    // Two unlabeled rows declared but one drawn: the missing row may still
    // be on the next page, so nothing is recovered yet.
    const partial = labelsBoard({ None: { rows: [plain], total: 2 } });
    const fallback = flatBoard({ ...flatEnvelope, results: [kept, plain, ghost], total_count: 3 });
    expect(reconcileLabelGhosts(partial, fallback, "main")).toBe(partial);
    // Once the column holds its whole window the omission is a ghost.
    const settled = labelsBoard({
      [LABEL]: { rows: [kept], total: 1 },
      None: { rows: [plain, ghost], total: 2 },
    });
    const otherGhost = row({ id: "other-ghost", label_ids: [] });
    const wider = flatBoard({
      ...flatEnvelope,
      results: [kept, plain, ghost, otherGhost],
      total_count: 4,
    });
    const result = reconcileLabelGhosts(settled, wider, "main");
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([
      [LABEL, 1],
      ["None", 3],
    ]);
    expect(result.lanes[0]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["plain", "ghost", "other-ghost"]);
  });

  it("places a ghost under the lane the fallback read put it in", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const plainB = row({ id: "plain-b", state_id: laneB, label_ids: [] });
    const ghost = row({ id: "ghost", state_id: laneB, label_ids: [] });
    const board = subGroupedBoard(
      subGrouped({
        None: {
          [laneA]: { rows: [plainA], total: 1 },
          [laneB]: { rows: [plainB], total: 1 },
        },
      }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: new Map([
          [laneA, "Todo"],
          [laneB, "Done"],
        ]),
        showEmpty: true,
      }
    );
    expect(board.columns.map((column) => column.key)).toEqual(["None"]);
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA], total: 1 },
        [laneB]: { rows: [plainB, ghost], total: 2 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "main");
    // The board drew two cards under None (one per lane); the fallback
    // says lane B holds two unlabeled rows, so the column grows to three.
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([["None", 3]]);
    expect(result.lanes.map((entry) => [entry.key, entry.cells["None"]?.rows.map((item) => item.id)])).toEqual([
      [laneA, ["plain-a"]],
      [laneB, ["plain-b", "ghost"]],
    ]);
    expect(result.lanes[1]?.cells["None"]?.total).toBe(2);
    expect(result.subGrouped).toBe(true);
  });

  it("never draws a lane the labels board does not have", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const board = subGroupedBoard(subGrouped({ None: { [laneA]: { rows: [plainA], total: 1 } } }), {
      groupDimension: "labels",
      subDimension: "state",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    const fallback = groupedBoard(grouped({ [laneB]: { rows: [row({ id: "ghost", state_id: laneB })], total: 1 } }), {
      dimension: "state",
      labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
  });

  it("leaves the board alone without a fallback lane or any unlabeled row", () => {
    const kept = row({ id: "kept", label_ids: [LABEL] });
    const board = labelsBoard({ [LABEL]: { rows: [kept], total: 1 } });
    expect(reconcileLabelGhosts(board, { ...board, lanes: [] }, "main")).toBe(board);
    // A fallback whose rows all carry a live label has nothing to recover.
    const fallback = flatBoard({ ...flatEnvelope, results: [kept], total_count: 1 });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
  });

  it("keeps a lane with no None cell out of the recovered counts", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const board = subGroupedBoard(
      subGrouped({
        None: {
          [laneA]: { rows: [plainA], total: 1 },
          [laneB]: { rows: [], total: 0 },
        },
      }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const ghost = row({ id: "ghost", state_id: laneB, label_ids: [] });
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA], total: 1 },
        [laneB]: { rows: [ghost], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.lanes.map((entry) => [entry.key, entry.cells["None"]?.rows.map((item) => item.id)])).toEqual([
      [laneA, ["plain-a"]],
      [laneB, ["ghost"]],
    ]);
    expect(result.columns.map((column) => column.total)).toEqual([2]);
  });

  it("recovers a ghost into a lane with no None cell yet", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const board = subGroupedBoard(
      subGrouped({ None: { [laneA]: { rows: [plainA], total: 1 }, [laneB]: { rows: [], total: 0 } } }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const laneBView = board.lanes[1]!;
    delete laneBView.cells["None"];
    const ghost = row({ id: "ghost", state_id: laneB, label_ids: [] });
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA], total: 1 },
        [laneB]: { rows: [ghost], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.lanes[1]?.cells["None"]?.rows.map((entry) => entry.id)).toEqual(["ghost"]);
    expect(result.lanes[1]?.cells["None"]?.total).toBe(1);
    expect(result.columns.map((column) => column.total)).toEqual([2]);
  });

  it("counts a paging lane's None cell alongside a lane that recovered", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const b1 = row({ id: "b1", state_id: laneB, label_ids: [] });
    const b2 = row({ id: "b2", state_id: laneB, label_ids: [] });
    // Lane A holds its whole None window; lane B has one card still to page
    // in. The board's None column already counts both lanes' totals.
    const board = subGroupedBoard(
      subGrouped({
        None: {
          [laneA]: { rows: [plainA], total: 1 },
          [laneB]: { rows: [b1], total: 2 },
        },
      }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    expect(board.columns.map((column) => column.total)).toEqual([3]);
    const ghost = row({ id: "ghost", state_id: laneA, label_ids: [] });
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA, ghost], total: 2 },
        [laneB]: { rows: [b1, b2], total: 2 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "main");
    // Lane A recovers its ghost and now holds two; lane B is still paging
    // and keeps its own cards. The header covers both lanes, so it equals
    // the cards behind it once lane B pages in rather than only lane A's.
    expect(result.lanes.map((entry) => [entry.key, entry.cells["None"]?.rows.map((item) => item.id)])).toEqual([
      [laneA, ["plain-a", "ghost"]],
      [laneB, ["b1"]],
    ]);
    expect(result.lanes.map((entry) => entry.cells["None"]?.total)).toEqual([2, 2]);
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([["None", 4]]);
    expect(result.total).toBe(4);
  });

  it("counts a lane with no None cell as zero", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const keptB = row({ id: "kept-b", state_id: laneB, label_ids: [LABEL] });
    // Lane B carries only a live label, so nothing ever draws a None cell
    // for it.
    const board = subGroupedBoard(
      subGrouped({
        [LABEL]: { [laneB]: { rows: [keptB], total: 1 } },
        None: { [laneA]: { rows: [plainA], total: 1 } },
      }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    delete board.lanes.find((entry) => entry.key === laneB)!.cells["None"];
    const ghost = row({ id: "ghost", state_id: laneA, label_ids: [] });
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA, ghost], total: 2 },
        [laneB]: { rows: [keptB], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.lanes.find((entry) => entry.key === laneA)?.cells["None"]?.total).toBe(2);
    expect(result.lanes.find((entry) => entry.key === laneB)?.cells["None"]).toBeUndefined();
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([
      [LABEL, 1],
      ["None", 2],
    ]);
  });

  it("keeps a lane whose fallback holds no unlabeled row as it is", () => {
    const laneA = "623e4567-e89b-12d3-a456-426614174005";
    const laneB = "723e4567-e89b-12d3-a456-426614174006";
    const plainA = row({ id: "plain-a", state_id: laneA, label_ids: [] });
    const keptB = row({ id: "kept-b", state_id: laneB, label_ids: [LABEL] });
    const board = subGroupedBoard(
      subGrouped({
        [LABEL]: { [laneB]: { rows: [keptB], total: 1 } },
        None: { [laneA]: { rows: [plainA], total: 1 }, [laneB]: { rows: [], total: 0 } },
      }),
      {
        groupDimension: "labels",
        subDimension: "state",
        groupLabels: labels,
        subLabels: labels,
        showEmpty: true,
      }
    );
    const ghost = row({ id: "ghost", state_id: laneA, label_ids: [] });
    const fallback = groupedBoard(
      grouped({
        [laneA]: { rows: [plainA, ghost], total: 2 },
        [laneB]: { rows: [keptB], total: 1 },
      }),
      { dimension: "state", labels, showEmpty: true }
    );
    const before = board.lanes.find((entry) => entry.key === laneB)!;
    const result = reconcileLabelGhosts(board, fallback, "main");
    expect(result.lanes.find((entry) => entry.key === laneB)).toBe(before);
    const laneACell = result.lanes.find((entry) => entry.key === laneA)?.cells["None"];
    expect(laneACell?.rows.map((entry) => entry.id)).toEqual(["plain-a", "ghost"]);
    expect(laneACell?.total).toBe(2);
    expect(result.columns.map((column) => [column.key, column.total])).toEqual([
      [LABEL, 1],
      ["None", 2],
    ]);
  });

  it("ignores a sub-grouped fallback, which no labels board reads", () => {
    const ghost = row({ id: "ghost", label_ids: [] });
    const board = labelsBoard({ None: { rows: [row({ id: "plain" })], total: 1 } });
    const fallback = subGroupedBoard(subGrouped({ None: { None: { rows: [ghost], total: 1 } } }), {
      groupDimension: "labels",
      subDimension: "state",
      groupLabels: labels,
      subLabels: labels,
      showEmpty: true,
    });
    expect(reconcileLabelGhosts(board, fallback, "main")).toBe(board);
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

  it("carries a lane's target date into the create prefill", () => {
    expect(boardPrefill("state", STATE, "target_date", "2026-12-01")).toEqual({
      state_id: STATE,
      target_date: "2026-12-01",
    });
    expect(boardCreateInitialValues(boardPrefill("state", STATE, "target_date", "2026-12-01"))).toEqual({
      stateId: STATE,
      targetDate: "2026-12-01",
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
