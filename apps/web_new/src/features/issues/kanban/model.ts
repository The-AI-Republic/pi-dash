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
  SubGroupedIssueBucket,
  SubGroupedIssueListResponse,
} from "@pidash/api-client";

import { GROUP_NONE_LABEL } from "../display/matrix.js";
import type { CreateFormInitialValues } from "../create/createForm.js";
import {
  FLAT_GROUP_TITLE,
  NONE_GROUP_KEY,
  groupTitle,
  normalizeGroupedRow,
  orderGroupKeys,
  sectionPrefill,
} from "../list/grouping.js";
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
 * Dimensions whose project value list the grouped read can leave out:
 * the server only emits buckets for used labels, modules and cycles.
 * With empty groups visible the project's own values join the board, so
 * an unused value still gets its column or lane. The empty-value bucket
 * is synthetic for these dimensions too, because a fully assigned board
 * carries no None bucket.
 */
const REFERENCE_FILLED_DIMENSIONS: ReadonlySet<string> = new Set(["labels", "module", "cycle", "assignees"]);

/**
 * Group keys to draw: the envelope's own keys in its order, then the
 * project values the envelope did not mention, then the empty-value
 * bucket last. Filling only happens while empty groups are visible; with
 * the flag off zero-count groups are dropped anyway, so the envelope's
 * keys remain the whole domain.
 */
function filledGroupKeys(
  keys: readonly string[],
  options: { dimension: string | null; showEmpty: boolean; domain?: readonly string[] | undefined }
): { keys: string[]; synthetic: ReadonlySet<string> } {
  if (!options.showEmpty) return { keys: orderGroupKeys(keys), synthetic: new Set<string>() };
  const seen = new Set(keys);
  const synthetic = new Set<string>();
  const filled: string[] = [...keys];
  for (const value of options.domain ?? []) {
    if (value === NONE_GROUP_KEY || seen.has(value)) continue;
    seen.add(value);
    synthetic.add(value);
    filled.push(value);
  }
  if (options.dimension !== null && REFERENCE_FILLED_DIMENSIONS.has(options.dimension) && !seen.has(NONE_GROUP_KEY)) {
    synthetic.add(NONE_GROUP_KEY);
    filled.push(NONE_GROUP_KEY);
  }
  return { keys: orderGroupKeys(filled), synthetic };
}

/**
 * Reconciled bucket total (NEWFRONT-153): the grouped count filter can
 * exclude rows the grouped results include (the pending-triage issue),
 * so the server total undercounts the cards. The header count equals the
 * number of issues in the group, hence at least the rows already seen.
 */
export function bucketTotal(bucket: Pick<IssueGroupBucket, "results" | "total_results">): number {
  return Math.max(bucket.total_results, bucket.results.length);
}

/**
 * Reconciled group total of a sub-grouped bucket: the server group total
 * already sums its sub-buckets, but each sub-bucket total undercounts the
 * same way grouped buckets do, so the reconciled sub-buckets win unless
 * the server total counts rows beyond them.
 */
function subGroupTotal(bucket: Pick<SubGroupedIssueBucket, "results" | "total_results">): number {
  let rows = 0;
  for (const sub of Object.values(bucket.results)) rows += bucketTotal(sub);
  return Math.max(bucket.total_results, rows);
}

/**
 * Columns from a grouped envelope: the server's declared order, the
 * synthetic empty-value bucket last, each with its own full count. The
 * rows of the first page ride their own cells, so callers can render a
 * flat board from the same shape.
 */
export function boardColumns(
  response: Pick<GroupedIssueListResponse, "results">,
  labels: ReadonlyMap<string, string>,
  options: { dimension: string | null; showEmpty: boolean; domain?: readonly string[] | undefined }
): BoardColumn[] {
  const columns: BoardColumn[] = [];
  const filled = filledGroupKeys(Object.keys(response.results), options);
  for (const key of filled.keys) {
    const bucket = response.results[key] as IssueGroupBucket | undefined;
    if (bucket === undefined) {
      if (!filled.synthetic.has(key)) continue;
      columns.push({
        key,
        title: groupTitle(key, labels, { dimension: options.dimension }),
        total: 0,
      });
      continue;
    }
    const total = bucketTotal(bucket);
    if (!options.showEmpty && total === 0) continue;
    columns.push({
      key,
      title: groupTitle(key, labels, { dimension: options.dimension }),
      total,
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
      total: bucketTotal(bucket),
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

/**
 * Sub-group keys in first-seen server order with the empty bucket last.
 * With empty groups visible, a reference-backed sub-group (labels,
 * modules, cycles) also draws its project values and a synthetic empty
 * lane, since the envelope only carries used values.
 */
export function laneKeys(
  response: Pick<SubGroupedIssueListResponse, "results">,
  showEmpty: boolean,
  options: { dimension?: string | null; domain?: readonly string[] | undefined } = {}
): string[] {
  const seen = new Set<string>();
  for (const bucket of Object.values(response.results)) {
    for (const key of Object.keys(bucket.results)) seen.add(key);
  }
  const ordered = filledGroupKeys([...seen], {
    dimension: options.dimension ?? null,
    showEmpty,
    domain: options.domain,
  }).keys;
  if (showEmpty) return ordered;
  return ordered.filter((key) => laneTotal(response, key) > 0);
}

/** Cumulative count of one sub-group value across every column. */
export function laneTotal(response: Pick<SubGroupedIssueListResponse, "results">, laneKey: string): number {
  let total = 0;
  for (const bucket of Object.values(response.results)) {
    const sub = bucket.results[laneKey];
    total += sub === undefined ? 0 : bucketTotal(sub);
  }
  return total;
}

/**
 * Columns of a sub-grouped envelope. A column's count is the server's
 * group total reconciled with its sub-buckets' rendered rows, since the
 * sub-bucket totals undercount the same way grouped buckets do.
 */
export function subGroupedColumns(
  response: Pick<SubGroupedIssueListResponse, "results">,
  labels: ReadonlyMap<string, string>,
  options: { dimension: string | null; showEmpty: boolean; domain?: readonly string[] | undefined }
): BoardColumn[] {
  const columns: BoardColumn[] = [];
  const filled = filledGroupKeys(Object.keys(response.results), options);
  for (const key of filled.keys) {
    const bucket = response.results[key];
    if (bucket === undefined) {
      if (!filled.synthetic.has(key)) continue;
      columns.push({
        key,
        title: groupTitle(key, labels, { dimension: options.dimension }),
        total: 0,
      });
      continue;
    }
    const total = subGroupTotal(bucket);
    if (!options.showEmpty && total === 0) continue;
    columns.push({
      key,
      title: groupTitle(key, labels, { dimension: options.dimension }),
      total,
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
    domain?: readonly string[] | undefined;
  }
): BoardLane[] {
  const lanes: BoardLane[] = [];
  for (const laneKey of laneKeys(response, options.showEmpty, {
    dimension: options.subDimension,
    domain: options.domain,
  })) {
    const cells: Record<string, BoardCell> = {};
    for (const column of options.columns) {
      const bucket = response.results[column.key]?.results[laneKey];
      cells[column.key] = {
        columnKey: column.key,
        rows:
          bucket === undefined
            ? [...NO_ROWS]
            : bucket.results.map((row) => normalizeGroupedRow(row, options.groupDimension)),
        total: bucket === undefined ? 0 : bucketTotal(bucket),
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
    groupDomain?: readonly string[] | undefined;
    subDomain?: readonly string[] | undefined;
  }
): BoardData {
  const columns = subGroupedColumns(response, options.groupLabels, {
    dimension: options.groupDimension,
    showEmpty: options.showEmpty,
    domain: options.groupDomain,
  });
  return {
    columns,
    lanes: subGroupedLanes(response, {
      groupDimension: options.groupDimension,
      subDimension: options.subDimension,
      labels: options.subLabels,
      columns,
      showEmpty: options.showEmpty,
      domain: options.subDomain,
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
    domain?: readonly string[] | undefined;
  }
): BoardData {
  const columns = boardColumns(response, options.labels, options);
  const cells = boardCells(response, options.dimension);
  const visibleCells: Record<string, BoardCell> = {};
  for (const column of columns) {
    visibleCells[column.key] = cells[column.key] ?? { columnKey: column.key, rows: [...NO_ROWS], total: 0 };
  }
  return {
    columns,
    lanes: [
      {
        key: "",
        title: "",
        total: response.total_count,
        cells: visibleCells,
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
 * The column or cell that asked for a later page. Flat pages have no
 * owner and keep merging whole; column and cell pages merge only into
 * the owner, so uneven columns never inherit each other's windows.
 */
export type BoardPageOwner =
  | { kind: "column"; columnKey: string }
  | { kind: "cell"; columnKey: string; laneKey: string };

/**
 * Fold a later page into the board: rows append per cell without
 * duplicates, and every total refreshes from the latest envelope. Cells,
 * lanes and columns the first page never discovered join the tail.
 * With an owner, only the owner's rows append and only the owner joins
 * the tail: other cells keep their rows (totals still refresh, since
 * every envelope carries full counts), and a newly discovered non-owner
 * bucket stays out rather than joining with a partial window. Every
 * merged total stays at or above the rows it covers, so an undercounted
 * server total never drops a header below its rendered cards.
 */
export function mergeBoardPage(current: BoardData, incoming: BoardData, owner?: BoardPageOwner): BoardData {
  const ownsCell = (laneKey: string, columnKey: string): boolean => {
    if (owner === undefined) return true;
    if (columnKey !== owner.columnKey) return false;
    if (owner.kind === "cell") return laneKey === owner.laneKey;
    return laneKey === "";
  };
  const ownsLane = (laneKey: string): boolean => {
    if (owner === undefined) return true;
    if (owner.kind === "column") return false;
    return laneKey === owner.laneKey;
  };
  const ownsColumn = (columnKey: string): boolean => {
    if (owner === undefined) return true;
    return columnKey === owner.columnKey;
  };
  const columns = current.columns.map((column) => {
    const next = incoming.columns.find((entry) => entry.key === column.key);
    return next === undefined ? column : { ...column, total: next.total };
  });
  for (const column of incoming.columns) {
    if (columns.some((entry) => entry.key === column.key)) continue;
    if (!ownsColumn(column.key)) continue;
    columns.push(column);
  }
  const lanes = current.lanes.map((lane) => {
    const next = incoming.lanes.find((entry) => entry.key === lane.key);
    if (next === undefined) return lane;
    const cells = { ...lane.cells };
    for (const [key, cell] of Object.entries(next.cells)) {
      const existing = cells[key];
      if (existing === undefined) {
        if (ownsCell(lane.key, key)) cells[key] = cell;
        continue;
      }
      if (!ownsCell(lane.key, key)) {
        cells[key] = { ...existing, total: Math.max(cell.total, existing.rows.length) };
        continue;
      }
      const seen = new Set(existing.rows.map((row) => row.id));
      const mergedRows = [...existing.rows, ...cell.rows.filter((row) => !seen.has(row.id))];
      cells[key] = {
        ...existing,
        rows: mergedRows,
        total: Math.max(cell.total, mergedRows.length),
      };
    }
    let laneRows = 0;
    for (const cell of Object.values(cells)) laneRows += cell.rows.length;
    return { ...lane, cells, total: Math.max(next.total, laneRows) };
  });
  for (const lane of incoming.lanes) {
    if (lanes.some((entry) => entry.key === lane.key)) continue;
    if (!ownsLane(lane.key)) continue;
    const cells = Object.fromEntries(Object.entries(lane.cells).filter(([key]) => ownsCell(lane.key, key)));
    lanes.push({ ...lane, cells });
  }
  const reconciled = columns.map((column) => {
    let rendered = 0;
    for (const lane of lanes) rendered += lane.cells[column.key]?.rows.length ?? 0;
    return { ...column, total: Math.max(column.total, rendered) };
  });
  return { columns: reconciled, lanes, subGrouped: current.subGrouped, total: incoming.total };
}

/**
 * Fold rows the labels sub-grouped read omits back into the None lane
 * (ISS-031/NEWFRONT-158). A deleted label leaves a soft-deleted join row:
 * the server's labels grouping filters it out of every lane and out of
 * the envelope total, while the flat read still shows the issue as
 * unlabeled. Nothing in the sub-grouped envelope marks the omission, so
 * the caller always reads the board once without the label axis and this
 * merge decides what is missing.
 *
 * The fallback read is grouped only by the board's main dimension. A
 * column is trusted only when its fallback rows and every lane's window
 * for it are complete, so a partially paged column is never mistaken for
 * a ghost. Unlabeled rows
 * the board does not already show join None; the fallback's unlabeled
 * count is the count authority for the cell, and a column the ghost
 * emptied joins the board so its row has somewhere to render.
 */
export function reconcileLabelGhosts(board: BoardData, fallback: BoardData): BoardData {
  if (!board.subGrouped || fallback.subGrouped) return board;
  const fallbackLane = fallback.lanes[0];
  if (fallbackLane === undefined) return board;
  const recovered = new Map<string, { rows: IssueListItem[]; unlabeled: number; total: number }>();
  const known = new Set(board.columns.map((column) => column.key));
  const candidates = [...board.columns, ...fallback.columns.filter((column) => !known.has(column.key))];
  for (const column of candidates) {
    const fallbackCell = fallbackLane.cells[column.key];
    if (fallbackCell === undefined || fallbackCell.rows.length < fallbackCell.total) continue;
    // Every lane must hold its whole window for this column before a row
    // missing from the board can be called ghosted rather than simply not
    // paged in yet.
    const settled = board.lanes.every((lane) => {
      const cell = lane.cells[column.key];
      return cell === undefined || cell.rows.length >= cell.total;
    });
    if (!settled) continue;
    const unlabeled = fallbackCell.rows.filter((row) => row.label_ids.length === 0);
    const present = new Set<string>();
    for (const lane of board.lanes) {
      for (const row of lane.cells[column.key]?.rows ?? []) present.add(row.id);
    }
    const missing = unlabeled.filter((row) => !present.has(row.id));
    if (missing.length === 0) continue;
    recovered.set(column.key, { rows: missing, unlabeled: unlabeled.length, total: fallbackCell.total });
  }
  if (recovered.size === 0) return board;
  // A column only the fallback read knows joins the board when it actually
  // recovered a row; otherwise it would carry a count with no cards.
  const columns = [
    ...board.columns,
    ...fallback.columns.filter((column) => !known.has(column.key) && recovered.has(column.key)),
  ].map((column) => {
    const entry = recovered.get(column.key);
    return entry === undefined ? column : { ...column, total: entry.total };
  });
  const growNone = (lane: BoardLane): BoardLane => {
    const cells = { ...lane.cells };
    for (const [key, entry] of recovered) {
      const rows = [...(cells[key]?.rows ?? NO_ROWS), ...entry.rows];
      cells[key] = { columnKey: key, rows, total: Math.max(entry.unlabeled, rows.length) };
    }
    return { ...lane, cells, total: Object.values(cells).reduce((sum, cell) => sum + cell.total, 0) };
  };
  const lanes = board.lanes.map((lane) => (lane.key === "None" ? growNone(lane) : lane));
  if (!lanes.some((lane) => lane.key === "None")) {
    const cells: Record<string, BoardCell> = {};
    for (const [key, entry] of recovered) {
      cells[key] = { columnKey: key, rows: [...entry.rows], total: Math.max(entry.unlabeled, entry.rows.length) };
    }
    lanes.push({
      key: "None",
      title: GROUP_NONE_LABEL,
      total: Object.values(cells).reduce((sum, cell) => sum + cell.total, 0),
      cells,
    });
  }
  return {
    columns,
    lanes,
    subGrouped: true,
    total: Math.max(
      board.total,
      columns.reduce((sum, column) => sum + column.total, 0)
    ),
  };
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
  if (typeof prefill["target_date"] === "string") initial.targetDate = prefill["target_date"];
  return initial;
}
