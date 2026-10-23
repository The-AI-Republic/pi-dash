// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Kanban board model (parity/specs/issues.md §5, ISS-028–035). Turns a
// grouped, sub-grouped or flat issue envelope into the columns the board
// draws and, when sub-grouped, the lanes those columns repeat per value.
// Column titles reuse the list grouping vocabulary; the empty-value
// bucket is always named "None" and sorts last. A lane's count is the
// number of issues across every column, and the empty-group display
// flag drops zero-count columns and lanes on both axes. Quick-add
// payloads merge the column's value with the lane's value so a created
// item lands in the cell it was typed into.

import type {
  GroupedIssueListResponse,
  IssueGroupBucket,
  IssueListItem,
  IssueListResponse,
  SubGroupedIssueListResponse,
} from "@pidash/api-client";

import { GROUP_NONE_LABEL } from "../display/matrix.js";
import type { CreateFormInitialValues } from "../create/createForm.js";
import { FLAT_GROUP_TITLE, groupTitle, normalizeGroupedRow, orderGroupKeys, sectionPrefill } from "../list/grouping.js";
import type { IssueGroupSection } from "../list/grouping.js";

/** Issues read per board column (spec §5: thirty per page). */
export const BOARD_PAGE_SIZE = 30;
/** Issues read per board cell when a sub-group is active (ten per page). */
export const BOARD_SUB_PAGE_SIZE = 10;

/** One column's identity and full issue count. */
export interface BoardColumn {
  /** Server bucket key (`state_id` value, `None`, or `all` when flat). */
  key: string;
  title: string;
  /** Every issue in the column, across all lanes and pages. */
  total: number;
}

/** One cell of a lane: the column it sits in and the rows loaded so far. */
export interface BoardCell {
  columnKey: string;
  rows: IssueListItem[];
  total: number;
}

/** One swimlane: a sub-group value repeated across every column. */
export interface BoardLane {
  key: string;
  title: string;
  /** Cumulative issue count across the lane's columns. */
  total: number;
  cells: Record<string, BoardCell>;
}

/** The whole board: columns, lanes (a single unnamed lane when flat). */
export interface BoardData {
  columns: BoardColumn[];
  lanes: BoardLane[];
  /** True when a sub-group dimension produced lanes. */
  subGrouped: boolean;
  /** Server total across every column. */
  total: number;
}

/** Empty row list shared by empty cells. */
const NO_ROWS: readonly IssueListItem[] = [];

/**
 * Columns from a grouped envelope: the server's declared order, the
 * synthetic empty-value bucket last, each with its own full count. The
 * rows of the first page ride their own cells, so callers can render a
 * flat board from the same shape.
 */
export function boardColumns(
  response: Pick<GroupedIssueListResponse, "results">,
  labels: ReadonlyMap<string, string>,
  options: { dimension: string | null; showEmpty: boolean }
): BoardColumn[] {
  const columns: BoardColumn[] = [];
  for (const key of orderGroupKeys(Object.keys(response.results))) {
    const bucket = response.results[key] as IssueGroupBucket | undefined;
    if (bucket === undefined) continue;
    if (!options.showEmpty && bucket.total_results === 0) continue;
    columns.push({
      key,
      title: groupTitle(key, labels, { dimension: options.dimension }),
      total: bucket.total_results,
    });
  }
  return columns;
}

/** First-page rows per column of a grouped envelope. */
export function boardCells(
  response: Pick<GroupedIssueListResponse, "results">,
  dimension: string | null
): Record<string, BoardCell> {
  const cells: Record<string, BoardCell> = {};
  for (const [key, bucket] of Object.entries(response.results)) {
    cells[key] = {
      columnKey: key,
      rows: bucket.results.map((row) => normalizeGroupedRow(row, dimension)),
      total: bucket.total_results,
    };
  }
  return cells;
}

/** A flat envelope's single column, titled like the list's flat section. */
export function flatBoard(response: Pick<IssueListResponse, "results" | "total_count">): BoardData {
  return {
    columns: [{ key: "all", title: FLAT_GROUP_TITLE, total: response.total_count }],
    lanes: [
      {
        key: "",
        title: "",
        total: response.total_count,
        cells: { all: { columnKey: "all", rows: [...response.results], total: response.total_count } },
      },
    ],
    subGrouped: false,
    total: response.total_count,
  };
}

/** Sub-group keys in first-seen server order with the empty bucket last. */
export function laneKeys(response: Pick<SubGroupedIssueListResponse, "results">, showEmpty: boolean): string[] {
  const seen = new Set<string>();
  for (const bucket of Object.values(response.results)) {
    for (const key of Object.keys(bucket.results)) seen.add(key);
  }
  const ordered = orderGroupKeys([...seen]);
  if (showEmpty) return ordered;
  return ordered.filter((key) => laneTotal(response, key) > 0);
}

/** Cumulative count of one sub-group value across every column. */
export function laneTotal(response: Pick<SubGroupedIssueListResponse, "results">, laneKey: string): number {
  let total = 0;
  for (const bucket of Object.values(response.results)) {
    total += bucket.results[laneKey]?.total_results ?? 0;
  }
  return total;
}

/**
 * Columns of a sub-grouped envelope. A column's count is the server's
 * group total, which already sums its sub-buckets.
 */
export function subGroupedColumns(
  response: Pick<SubGroupedIssueListResponse, "results">,
  labels: ReadonlyMap<string, string>,
  options: { dimension: string | null; showEmpty: boolean }
): BoardColumn[] {
  const columns: BoardColumn[] = [];
  for (const key of orderGroupKeys(Object.keys(response.results))) {
    const bucket = response.results[key];
    if (bucket === undefined) continue;
    if (!options.showEmpty && bucket.total_results === 0) continue;
    columns.push({
      key,
      title: groupTitle(key, labels, { dimension: options.dimension }),
      total: bucket.total_results,
    });
  }
  return columns;
}

/** Lanes with their first-page cells for every rendered column. */
export function subGroupedLanes(
  response: Pick<SubGroupedIssueListResponse, "results">,
  options: {
    groupDimension: string | null;
    subDimension: string | null;
    labels: ReadonlyMap<string, string>;
    columns: readonly BoardColumn[];
    showEmpty: boolean;
  }
): BoardLane[] {
  const lanes: BoardLane[] = [];
  for (const laneKey of laneKeys(response, options.showEmpty)) {
    const cells: Record<string, BoardCell> = {};
    for (const column of options.columns) {
      const bucket = response.results[column.key]?.results[laneKey];
      cells[column.key] = {
        columnKey: column.key,
        rows:
          bucket === undefined
            ? [...NO_ROWS]
            : bucket.results.map((row) => normalizeGroupedRow(row, options.groupDimension)),
        total: bucket?.total_results ?? 0,
      };
    }
    lanes.push({
      key: laneKey,
      title: groupTitle(laneKey, options.labels, { dimension: options.subDimension }),
      total: laneTotal(response, laneKey),
      cells,
    });
  }
  return lanes;
}

/** Whole board from a sub-grouped envelope. */
export function subGroupedBoard(
  response: SubGroupedIssueListResponse,
  options: {
    groupDimension: string | null;
    subDimension: string | null;
    groupLabels: ReadonlyMap<string, string>;
    subLabels: ReadonlyMap<string, string>;
    showEmpty: boolean;
  }
): BoardData {
  const columns = subGroupedColumns(response, options.groupLabels, {
    dimension: options.groupDimension,
    showEmpty: options.showEmpty,
  });
  return {
    columns,
    lanes: subGroupedLanes(response, {
      groupDimension: options.groupDimension,
      subDimension: options.subDimension,
      labels: options.subLabels,
      columns,
      showEmpty: options.showEmpty,
    }),
    subGrouped: true,
    total: response.total_count,
  };
}

/**
 * Whole board from a grouped envelope. A sub-group mode never reaches
 * here; it is a separate read with its own envelope.
 */
export function groupedBoard(
  response: GroupedIssueListResponse,
  options: {
    dimension: string | null;
    labels: ReadonlyMap<string, string>;
    showEmpty: boolean;
  }
): BoardData {
  const columns = boardColumns(response, options.labels, {
    dimension: options.dimension,
    showEmpty: options.showEmpty,
  });
  const cells = boardCells(response, options.dimension);
  const visible = new Set(columns.map((column) => column.key));
  return {
    columns,
    lanes: [
      {
        key: "",
        title: "",
        total: response.total_count,
        cells: Object.fromEntries(Object.entries(cells).filter(([key]) => visible.has(key))),
      },
    ],
    subGrouped: false,
    total: response.total_count,
  };
}

/**
 * Dimensions the board cannot mix: the server answers 400 when the
 * group and sub-group are the same field, so the stored record's
 * sub-group is ignored while the two match (ISS-028/029).
 */
export function effectiveSubGroup(groupBy: string | null, subGroupBy: string | null): string | null {
  if (groupBy === null || subGroupBy === null || groupBy === subGroupBy) return null;
  return subGroupBy;
}

/**
 * Quick-add payload for one cell: the column's grouping value merged
 * with the lane's value. Membership fields (cycle, module) ride along
 * the same keys; the create submit strips them and attaches after.
 */
export function boardPrefill(
  groupDimension: string | null,
  columnKey: string,
  subDimension: string | null,
  laneKey: string | null
): Record<string, string | string[]> {
  const column: IssueGroupSection = {
    key: columnKey,
    title: "",
    rows: [],
    total: 0,
    value: columnKey === "None" ? null : columnKey,
  };
  const prefill = groupDimension === null ? {} : sectionPrefill(groupDimension, column);
  if (subDimension === null || laneKey === null || laneKey === "") return prefill;
  const lane: IssueGroupSection = {
    key: laneKey,
    title: "",
    rows: [],
    total: 0,
    value: laneKey === "None" ? null : laneKey,
  };
  return { ...prefill, ...sectionPrefill(subDimension, lane) };
}

/** Why a board quick-add is unavailable, or undefined when offered. */
export interface BoardQuickAddGate {
  /** Project-aware edit standing (guests and archived projects are false). */
  canEdit: boolean;
  /** Completed cycles render read-only. */
  completedCycle?: boolean;
  /** Edition workflow rule reason; OSS never supplies one. */
  workflowBlockedReason?: string | undefined;
}

/**
 * The board offers quick-add in every grouping, including created-by
 * columns (ISS-035, row corrected against the old app). Permission and
 * the completed-cycle rule still suppress it.
 */
export function boardQuickAddDisabledReason(gate: BoardQuickAddGate): string | undefined {
  if (!gate.canEdit) return "New work items are read-only for you";
  if (gate.completedCycle === true) return "This cycle is complete";
  if (gate.workflowBlockedReason !== undefined) return gate.workflowBlockedReason;
  return undefined;
}

/** Whether a column or lane renders for the empty-group flag. */
export function showsGroup(total: number, showEmpty: boolean): boolean {
  return showEmpty || total > 0;
}

/** Empty-value bucket title, shared by columns and lanes. */
export const EMPTY_GROUP_TITLE = GROUP_NONE_LABEL;

/** Whether a column or cell still has unseen rows. */
export function boardHasMore(loaded: number, total: number): boolean {
  return loaded < total;
}

/** Cursor asking the server for the page that starts at `loaded` rows. */
export function boardPageCursor(loaded: number, pageSize: number): string {
  return `${pageSize}:${Math.floor(loaded / pageSize)}:0`;
}

/**
 * Fold a later page into the board: rows append per cell without
 * duplicates, and every total refreshes from the latest envelope. Cells,
 * lanes and columns the first page never discovered join the tail.
 */
export function mergeBoardPage(current: BoardData, incoming: BoardData): BoardData {
  const columns = current.columns.map((column) => {
    const next = incoming.columns.find((entry) => entry.key === column.key);
    return next === undefined ? column : { ...column, total: next.total };
  });
  for (const column of incoming.columns) {
    if (!columns.some((entry) => entry.key === column.key)) columns.push(column);
  }
  const lanes = current.lanes.map((lane) => {
    const next = incoming.lanes.find((entry) => entry.key === lane.key);
    if (next === undefined) return lane;
    const cells = { ...lane.cells };
    for (const [key, cell] of Object.entries(next.cells)) {
      const existing = cells[key];
      if (existing === undefined) {
        cells[key] = cell;
        continue;
      }
      const seen = new Set(existing.rows.map((row) => row.id));
      cells[key] = {
        ...existing,
        rows: [...existing.rows, ...cell.rows.filter((row) => !seen.has(row.id))],
        total: cell.total,
      };
    }
    return { ...lane, cells, total: next.total };
  });
  for (const lane of incoming.lanes) {
    if (!lanes.some((entry) => entry.key === lane.key)) lanes.push(lane);
  }
  return { columns, lanes, subGrouped: current.subGrouped, total: incoming.total };
}

/** Cell prefill translated to the create dialog's initial values. */
export function boardCreateInitialValues(prefill: Record<string, string | string[]>): CreateFormInitialValues {
  const initial: CreateFormInitialValues = {};
  if (typeof prefill["state_id"] === "string") initial.stateId = prefill["state_id"];
  if (typeof prefill["priority"] === "string") initial.priority = prefill["priority"];
  if (Array.isArray(prefill["assignee_ids"])) initial.assigneeIds = [...prefill["assignee_ids"]];
  if (Array.isArray(prefill["label_ids"])) initial.labelIds = [...prefill["label_ids"]];
  if (typeof prefill["cycle_id"] === "string") initial.cycleId = prefill["cycle_id"];
  if (Array.isArray(prefill["module_ids"])) initial.moduleIds = [...prefill["module_ids"]];
  return initial;
}
