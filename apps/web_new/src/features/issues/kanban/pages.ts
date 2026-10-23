// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board reads (parity/specs/issues.md §5, ISS-028/029). The board asks the
// grouped endpoint for thirty rows per column, or the sub-grouped one for
// ten per cell, at the active grouping dimension and the user's stored
// order. A project without grouping reads its flat list once. Later pages
// and per-column sentinels belong to the board-actions slice
// (NEWFRONT-623), so this module loads the first page of every bucket.

import { keepPreviousData, queryOptions, useQuery, type QueryKey } from "@tanstack/react-query";
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

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import { serverGroupBy } from "../list/grouping.js";
import {
  BOARD_PAGE_SIZE,
  BOARD_SUB_PAGE_SIZE,
  effectiveSubGroup,
  flatBoard,
  groupedBoard,
  subGroupedBoard,
  type BoardData,
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

function baseQuery(input: BoardReadInput, perPage: number): IssueListQuery {
  return withRichFilters(
    {
      perPage,
      orderBy: input.orderBy,
      filters: { sub_issue: String(input.showSubIssues === true) },
    },
    input.richFilters ?? {}
  );
}

/**
 * One board read. The sub-grouped endpoint needs both keys, the grouped
 * one only the group key, and without grouping the flat list answers.
 */
export async function readBoard(
  client: ApiClient,
  workspaceSlug: string,
  input: BoardReadInput
): Promise<BoardResponse> {
  const projectId = boardProjectId(input.entity);
  if (projectId === null) throw new Error("board reads need a project-scoped entity");
  const serverKey = serverGroupBy(input.groupBy);
  const subKey = serverGroupBy(effectiveSubGroup(input.groupBy, input.subGroupBy));
  const scope = input.entity.scope;
  if (serverKey === null) {
    const flat = baseQuery(input, BOARD_PAGE_SIZE);
    if (scope === "cycle") return listCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, flat);
    if (scope === "module") return listModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, flat);
    return listIssues(client, workspaceSlug, projectId, flat);
  }
  if (subKey !== null) {
    // The sub-grouped serializer is only mounted on the project list; the
    // cycle and module pages never offer a sub-group axis.
    return listSubGroupedIssues(client, workspaceSlug, projectId, {
      ...baseQuery(input, BOARD_SUB_PAGE_SIZE),
      groupBy: serverKey,
      subGroupBy: subKey,
    });
  }
  const query = { ...baseQuery(input, BOARD_PAGE_SIZE), groupBy: serverKey };
  if (scope === "cycle") {
    return listGroupedCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, query);
  }
  if (scope === "module") {
    return listGroupedModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, query);
  }
  return listGroupedIssues(client, workspaceSlug, projectId, query);
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

/** Read the board for one entity at the user's stored grouping. */
export function useBoard(client: ApiClient, workspaceSlug: string, input: BoardReadInput) {
  const query = useQuery(boardQueryOptions(client, workspaceSlug, input));
  return {
    board: query.data === undefined ? null : toBoard(query.data, input),
    isPending: query.isPending,
    isFetching: query.isFetching,
  };
}
