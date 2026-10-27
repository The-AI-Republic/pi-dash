// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board reads (parity/specs/issues.md §5, ISS-028/029, ISS-042). The board
// asks the grouped endpoint for thirty rows per column, or the sub-grouped
// one for ten per cell, at the active grouping dimension and the user's
// stored order. A project without grouping reads its flat list once. Each
// column and cell pages on its own afterwards: flat columns follow the
// flat cursor chain, grouped columns and sub-grouped cells ask for the
// page starting at their own row count, and every later page folds into
// the board without duplicating rows.

import { keepPreviousData, queryOptions, useQueries, useQuery, type QueryKey } from "@tanstack/react-query";
import {
  listCycleIssues,
  listGroupedCycleIssues,
  listGroupedIssues,
  listGroupedModuleIssues,
  listIssues,
  listModuleIssues,
  listSubGroupedIssues,
  type ApiClient,
  type GroupedIssueListResponse,
  type IssueListQuery,
  type IssueListResponse,
  type SubGroupedIssueListResponse,
} from "@pidash/api-client";
import * as React from "react";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import { serverGroupBy } from "../list/grouping.js";
import {
  BOARD_PAGE_SIZE,
  BOARD_SUB_PAGE_SIZE,
  boardHasMore,
  boardPageCursor,
  effectiveSubGroup,
  flatBoard,
  groupedBoard,
  mergeBoardPage,
  reconcileLabelGhosts,
  subGroupedBoard,
  type BoardCell,
  type BoardData,
  type BoardPageOwner,
} from "./model.js";

export interface BoardReadInput {
  entity: IssuesEntity;
  /** Display grouping dimension (state, priority, ...) or null. */
  groupBy: string | null;
  /** Display sub-grouping dimension or null. */
  subGroupBy: string | null;
  orderBy: string;
  showSubIssues?: boolean;
  showEmpty: boolean;
  richFilters?: RichExpression;
  /** Resolves group bucket ids to the header names they show. */
  groupLabels: ReadonlyMap<string, string>;
  /** Resolves sub-group bucket ids to the lane names they show. */
  subLabels: ReadonlyMap<string, string>;
}

/** The three envelope shapes a board read can answer. */
export type BoardResponse = IssueListResponse | GroupedIssueListResponse | SubGroupedIssueListResponse;

/** The project the entity is scoped to, or null without a scope. */
export function boardProjectId(entity: IssuesEntity): string | null {
  return "projectId" in entity ? entity.projectId : null;
}

function baseQuery(input: BoardReadInput, perPage: number, cursor?: string): IssueListQuery {
  return withRichFilters(
    {
      perPage,
      orderBy: input.orderBy,
      filters: { sub_issue: String(input.showSubIssues === true) },
      ...(cursor === undefined ? {} : { cursor }),
    },
    input.richFilters ?? {}
  );
}

/**
 * One board read. The sub-grouped endpoint needs both keys, the grouped
 * one only the group key, and without grouping the flat list answers. A
 * cursor asks for a later page of the same envelope.
 */
export async function readBoard(
  client: ApiClient,
  workspaceSlug: string,
  input: BoardReadInput,
  cursor?: string
): Promise<BoardResponse> {
  const projectId = boardProjectId(input.entity);
  if (projectId === null) throw new Error("board reads need a project-scoped entity");
  const serverKey = serverGroupBy(input.groupBy);
  const subKey = serverGroupBy(effectiveSubGroup(input.groupBy, input.subGroupBy));
  const scope = input.entity.scope;
  if (serverKey === null) {
    const flat = baseQuery(input, BOARD_PAGE_SIZE, cursor);
    if (scope === "cycle") return listCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, flat);
    if (scope === "module") return listModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, flat);
    return listIssues(client, workspaceSlug, projectId, flat);
  }
  if (subKey !== null) {
    // The sub-grouped serializer is only mounted on the project list; the
    // cycle and module pages never offer a sub-group axis.
    return listSubGroupedIssues(client, workspaceSlug, projectId, {
      ...baseQuery(input, BOARD_SUB_PAGE_SIZE, cursor),
      groupBy: serverKey,
      subGroupBy: subKey,
    });
  }
  const query = { ...baseQuery(input, BOARD_PAGE_SIZE, cursor), groupBy: serverKey };
  if (scope === "cycle") {
    return listGroupedCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, query);
  }
  if (scope === "module") {
    return listGroupedModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, query);
  }
  return listGroupedIssues(client, workspaceSlug, projectId, query);
}

/** Rows per recovery page while walking a label-grouped fallback. */
export const LABEL_RECOVERY_PAGE_SIZE = 100;

/**
 * Rows from a main-dimension grouped read, without the label axis. The
 * labels sub-grouped serializer can omit a row whose only label was
 * deleted (NEWFRONT-158); this read still carries it. It is project-scoped
 * because that is the only scope that offers sub-grouping, and it follows
 * the envelope's cursor chain so a column beyond the first window is not
 * mistaken for complete. A repeated cursor stops the walk.
 */
export async function readLabelRecovery(
  client: ApiClient,
  workspaceSlug: string,
  input: BoardReadInput
): Promise<GroupedIssueListResponse> {
  const projectId = boardProjectId(input.entity);
  if (projectId === null) throw new Error("label recovery needs a project-scoped entity");
  const groupBy = serverGroupBy(input.groupBy);
  if (groupBy === null) throw new Error("label recovery needs a grouped board");
  const buckets: GroupedIssueListResponse["results"] = {};
  const fold = (page: GroupedIssueListResponse) => {
    for (const [key, bucket] of Object.entries(page.results)) {
      const existing = buckets[key];
      if (existing === undefined) {
        buckets[key] = { results: [...bucket.results], total_results: bucket.total_results };
        continue;
      }
      const seen = new Set(existing.results.map((row) => row.id));
      buckets[key] = {
        results: [...existing.results, ...bucket.results.filter((row) => !seen.has(row.id))],
        total_results: bucket.total_results,
      };
    }
  };
  const firstPage = await listGroupedIssues(client, workspaceSlug, projectId, {
    ...baseQuery(input, LABEL_RECOVERY_PAGE_SIZE),
    groupBy,
  });
  fold(firstPage);
  const asked = new Set<string>();
  let page = firstPage;
  while (page.next_page_results && page.next_cursor !== "" && !asked.has(page.next_cursor)) {
    asked.add(page.next_cursor);
    page = await listGroupedIssues(client, workspaceSlug, projectId, {
      ...baseQuery(input, LABEL_RECOVERY_PAGE_SIZE, page.next_cursor),
      groupBy,
    });
    fold(page);
  }
  return { ...firstPage, results: buckets };
}

/** Grouped fallback read used to restore rows omitted by label grouping. */
export function labelRecoveryQueryOptions(client: ApiClient, workspaceSlug: string, input: BoardReadInput) {
  return queryOptions({
    queryKey: [...boardQueryKey(workspaceSlug, input), "label-recovery"],
    queryFn: () => readLabelRecovery(client, workspaceSlug, input),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Board query key; every axis that changes the envelope rides in it. */
export function boardQueryKey(workspaceSlug: string, input: BoardReadInput): QueryKey {
  return [
    ...issueKeys.lists(workspaceSlug, ...entityListsKeyPart(input.entity)),
    "board",
    input.groupBy ?? "flat",
    effectiveSubGroup(input.groupBy, input.subGroupBy) ?? "none",
    input.orderBy,
    input.showSubIssues === true,
    input.showEmpty,
    richFiltersParam(input.richFilters ?? {}),
  ];
}

export function boardQueryOptions(client: ApiClient, workspaceSlug: string, input: BoardReadInput) {
  return queryOptions({
    queryKey: boardQueryKey(workspaceSlug, input),
    queryFn: () => readBoard(client, workspaceSlug, input),
    staleTime: LIST_STALE_TIME_MS,
    placeholderData: keepPreviousData,
  });
}

/** Whether an envelope's results map group buckets to sub-buckets. */
export function isSubGrouped(response: BoardResponse): response is SubGroupedIssueListResponse {
  if (!("results" in response) || Array.isArray(response.results)) return false;
  const first = Object.values(response.results)[0] as unknown;
  return (
    typeof first === "object" &&
    first !== null &&
    "results" in first &&
    typeof first.results === "object" &&
    first.results !== null &&
    !Array.isArray(first.results)
  );
}

/** Whether an envelope's results map group keys to issue buckets. */
export function isGrouped(response: BoardResponse): response is GroupedIssueListResponse {
  if (!("results" in response) || Array.isArray(response.results)) return false;
  const first = Object.values(response.results)[0] as unknown;
  return typeof first === "object" && first !== null && "results" in first && Array.isArray(first.results);
}

/** Shape one read into the board the renderer draws. */
export function toBoard(response: BoardResponse, input: BoardReadInput): BoardData {
  if (isSubGrouped(response)) {
    return subGroupedBoard(response, {
      groupDimension: input.groupBy,
      subDimension: effectiveSubGroup(input.groupBy, input.subGroupBy),
      groupLabels: input.groupLabels,
      subLabels: input.subLabels,
      showEmpty: input.showEmpty,
    });
  }
  if (isGrouped(response)) {
    return groupedBoard(response, {
      dimension: input.groupBy,
      labels: input.groupLabels,
      showEmpty: input.showEmpty,
    });
  }
  return flatBoard(response as IssueListResponse);
}

/** Later-page read under the board's key. */
export function boardPageQueryOptions(client: ApiClient, workspaceSlug: string, input: BoardReadInput, cursor: string) {
  return queryOptions({
    queryKey: [...boardQueryKey(workspaceSlug, input), "page", cursor],
    queryFn: () => readBoard(client, workspaceSlug, input, cursor),
    staleTime: LIST_STALE_TIME_MS,
    placeholderData: keepPreviousData,
  });
}

/** One requested later page: its owner and cursor. */
interface BoardExtraPage {
  kind: "column" | "cell" | "flat";
  key: string;
  cursor: string;
}

/** Cell cursor owner: the lane and column the page was asked for. */
export function boardCellKey(columnKey: string, laneKey: string): string {
  return JSON.stringify([laneKey, columnKey]);
}

/** Split a cell cursor owner back into its lane and column. */
export function parseBoardCellKey(key: string): [laneKey: string, columnKey: string] {
  return JSON.parse(key) as [string, string];
}

/** The requesting owner of one extra page; flat pages merge whole. */
function pageOwner(entry: BoardExtraPage): BoardPageOwner | undefined {
  if (entry.kind === "flat") return undefined;
  if (entry.kind === "column") return { kind: "column", columnKey: entry.key };
  const [laneKey, columnKey] = parseBoardCellKey(entry.key);
  return { kind: "cell", columnKey, laneKey };
}

/** One cell of the merged board, or null before the first page lands. */
export function boardCellOf(board: BoardData | null, columnKey: string, laneKey: string | null): BoardCell | null {
  if (board === null) return null;
  const lane = laneKey === null ? board.lanes[0] : board.lanes.find((entry) => entry.key === laneKey);
  if (lane === undefined) return null;
  return lane.cells[columnKey] ?? null;
}

export interface BoardPaging {
  board: BoardData | null;
  isPending: boolean;
  isFetching: boolean;
  /** Ask for a grouped or flat column's next page. */
  loadMoreColumn: (columnKey: string) => void;
  /** Ask for a sub-grouped cell's next page. */
  loadMoreCell: (columnKey: string, laneKey: string) => void;
  /** Whether the column still has unseen rows. */
  columnHasMore: (columnKey: string) => boolean;
  /** Whether the cell still has unseen rows. */
  cellHasMore: (columnKey: string, laneKey: string) => boolean;
  /** Whether the column's next page is still in flight. */
  columnFetching: (columnKey: string) => boolean;
  /** Whether the cell's next page is still in flight. */
  cellFetching: (columnKey: string, laneKey: string) => boolean;
}

/**
 * Read the board for one entity at the user's stored grouping. The first
 * page discovers the buckets; every column and cell then pages on its
 * own — flat columns follow the flat cursor chain, grouped columns and
 * sub-grouped cells ask for the page starting at their own row count.
 * Asked cursors are remembered, so a column never loads the same page
 * twice and concurrent loads for one owner never stack.
 */
export function useBoard(client: ApiClient, workspaceSlug: string, input: BoardReadInput): BoardPaging {
  const resetKey = JSON.stringify(boardQueryKey(workspaceSlug, input));
  const [columnCursors, setColumnCursors] = React.useState<Record<string, string[]>>({});
  const [cellCursors, setCellCursors] = React.useState<Record<string, string[]>>({});
  const [flatCursors, setFlatCursors] = React.useState<string[]>([]);
  const [activeKey, setActiveKey] = React.useState(resetKey);
  if (activeKey !== resetKey) {
    setActiveKey(resetKey);
    setColumnCursors({});
    setCellCursors({});
    setFlatCursors([]);
  }

  const first = useQuery(boardQueryOptions(client, workspaceSlug, input));
  const subKey = serverGroupBy(effectiveSubGroup(input.groupBy, input.subGroupBy));
  const primaryBoard = React.useMemo(
    () => (first.data === undefined ? null : toBoard(first.data, input)),
    [first.data, input]
  );
  // The sub-grouped envelope carries no signal for a ghosted row: the
  // server drops it from the lanes and from the total alike. Only the
  // main-dimension read can show it, so labels boards always read one
  // (NEWFRONT-158).
  const recover = subKey === "labels__id" && input.entity.scope === "project";
  const extras = React.useMemo<BoardExtraPage[]>(
    () => [
      ...Object.entries(columnCursors).flatMap(([key, cursors]) =>
        cursors.map((cursor) => ({ kind: "column" as const, key, cursor }))
      ),
      ...Object.entries(cellCursors).flatMap(([key, cursors]) =>
        cursors.map((cursor) => ({ kind: "cell" as const, key, cursor }))
      ),
      ...flatCursors.map((cursor) => ({ kind: "flat" as const, key: "all", cursor })),
    ],
    [columnCursors, cellCursors, flatCursors]
  );
  const extraQueries = useQueries({
    queries: extras.map((entry) => boardPageQueryOptions(client, workspaceSlug, input, entry.cursor)),
  });
  const recovery = useQuery({
    ...labelRecoveryQueryOptions(client, workspaceSlug, input),
    enabled: recover,
  });

  const board = React.useMemo(() => {
    if (primaryBoard === null) return null;
    let merged = primaryBoard;
    for (const [index, query] of extraQueries.entries()) {
      if (query.data === undefined) continue;
      merged = mergeBoardPage(merged, toBoard(query.data, input), pageOwner(extras[index]!));
    }
    if (recover && recovery.data !== undefined) {
      merged = reconcileLabelGhosts(merged, toBoard(recovery.data, { ...input, subGroupBy: null }));
    }
    return merged;
  }, [primaryBoard, extraQueries, extras, input, recover, recovery.data, subKey]);

  const flatPages = React.useMemo(() => {
    const pages: BoardResponse[] = [];
    if (first.data !== undefined) pages.push(first.data);
    extras.forEach((entry, index) => {
      const data = extraQueries[index]?.data;
      if (entry.kind === "flat" && data !== undefined) pages.push(data);
    });
    return pages;
  }, [first.data, extras, extraQueries]);
  const flatLast = flatPages[flatPages.length - 1];
  const flatHasMore = flatLast === undefined ? false : flatLast.next_page_results;
  const flatNext = flatLast === undefined ? "" : flatLast.next_cursor;

  const loadMoreColumn = React.useCallback(
    (columnKey: string) => {
      const cell = boardCellOf(board, columnKey, null);
      if (cell === null || !boardHasMore(cell.rows.length, cell.total)) return;
      if (serverGroupBy(input.groupBy) === null) {
        if (!flatHasMore || flatNext === "") return;
        setFlatCursors((current) => (current.includes(flatNext) ? current : [...current, flatNext]));
        return;
      }
      const cursor = boardPageCursor(cell.rows.length, BOARD_PAGE_SIZE);
      setColumnCursors((current) => {
        const existing = current[columnKey] ?? [];
        if (existing.includes(cursor)) return current;
        return { ...current, [columnKey]: [...existing, cursor] };
      });
    },
    [board, flatHasMore, flatNext, input.groupBy]
  );

  const loadMoreCell = React.useCallback(
    (columnKey: string, laneKey: string) => {
      const cell = boardCellOf(board, columnKey, laneKey);
      if (cell === null || !boardHasMore(cell.rows.length, cell.total)) return;
      const cursor = boardPageCursor(cell.rows.length, BOARD_SUB_PAGE_SIZE);
      const key = boardCellKey(columnKey, laneKey);
      setCellCursors((current) => {
        const existing = current[key] ?? [];
        if (existing.includes(cursor)) return current;
        return { ...current, [key]: [...existing, cursor] };
      });
    },
    [board]
  );

  const fetching = React.useCallback(
    (kind: "column" | "cell" | "flat", key: string) =>
      extras.some(
        (entry, index) => entry.kind === kind && entry.key === key && extraQueries[index]?.isFetching === true
      ),
    [extras, extraQueries]
  );

  return {
    board,
    isPending: first.isPending,
    isFetching: first.isFetching || recovery.isFetching || extraQueries.some((query) => query.isFetching),
    loadMoreColumn,
    loadMoreCell,
    columnHasMore: (columnKey) => {
      const cell = boardCellOf(board, columnKey, null);
      return cell !== null && boardHasMore(cell.rows.length, cell.total);
    },
    cellHasMore: (columnKey, laneKey) => {
      const cell = boardCellOf(board, columnKey, laneKey);
      return cell !== null && boardHasMore(cell.rows.length, cell.total);
    },
    columnFetching: (columnKey) =>
      serverGroupBy(input.groupBy) === null ? fetching("flat", "all") : fetching("column", columnKey),
    cellFetching: (columnKey, laneKey) => fetching("cell", boardCellKey(columnKey, laneKey)),
  };
}
