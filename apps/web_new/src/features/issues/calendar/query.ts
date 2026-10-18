// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar reads (parity/specs/issues.md §4, ISS-021). One windowed,
// target-date-grouped read covers the visible grid; undated issues never
// arrive because the read filters on the date. Day buckets page through
// their own cursor so a crowded tile can reveal more rows.

import { queryOptions } from "@tanstack/react-query";
import {
  listStates,
  listGroupedCycleIssues,
  listGroupedIssues,
  listGroupedModuleIssues,
  type ApiClient,
  type GroupedIssueRow,
  type GroupedIssueListResponse,
  type IssueListItem,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import type { CalendarRange } from "./model.js";

/** Month tiles load four rows per day; week tiles show a fuller day. */
export const MONTH_PER_PAGE = 4;
export const WEEK_PER_PAGE = 30;

/** ISO days between two inclusive bounds, in order. */
export function daysBetween(after: string, before: string): string[] {
  const days: string[] = [];
  const start = new Date(`${after}T00:00:00`);
  const end = new Date(`${before}T00:00:00`);
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime()) || end < start) return days;
  for (let cursor = new Date(start); cursor <= end; cursor.setDate(cursor.getDate() + 1)) {
    days.push(
      `${cursor.getFullYear()}-${String(cursor.getMonth() + 1).padStart(2, "0")}-${String(cursor.getDate()).padStart(2, "0")}`
    );
  }
  return days;
}

/** The `target_date` filter value the server expects for a window. */
export function targetDateFilter(range: CalendarRange): string {
  return `${range.after};after,${range.before};before`;
}

/** Calendar query parameters for the visible range. */
export interface CalendarQuery {
  after: string;
  before: string;
  perPage: number;
}

/** Key beneath the entity's list prefix; the range is part of the key. */
export function calendarListKey(workspaceSlug: string, entity: IssuesEntity, query: CalendarQuery): readonly unknown[] {
  return [
    ...issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
    "calendar",
    query.after,
    query.before,
    query.perPage,
  ];
}

/** The entity-scoped read behind a calendar window. */
function readWindow(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  entity: IssuesEntity,
  query: CalendarQuery,
  cursor?: string
) {
  const input = {
    groupBy: "target_date" as const,
    perPage: query.perPage,
    ...(cursor === undefined ? {} : { cursor }),
    filters: { target_date: targetDateFilter(query) },
  };
  if (entity.scope === "cycle") {
    return listGroupedCycleIssues(client, workspaceSlug, projectId, entity.cycleId, input);
  }
  if (entity.scope === "module") {
    return listGroupedModuleIssues(client, workspaceSlug, projectId, entity.moduleId, input);
  }
  return listGroupedIssues(client, workspaceSlug, projectId, input);
}

/** Windowed grouped read for the calendar grid. */
export function calendarQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  entity: IssuesEntity,
  query: CalendarQuery
) {
  return queryOptions({
    queryKey: calendarListKey(workspaceSlug, entity, query),
    queryFn: () => readWindow(client, workspaceSlug, projectId, entity, query),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Flatten grouped buckets into rows without losing duplicates. */
export function rowsFromResponse(response: GroupedIssueListResponse): IssueListItem[] {
  const rows: IssueListItem[] = [];
  for (const bucket of Object.values(response.results)) {
    for (const row of bucket.results) rows.push(row as unknown as GroupedIssueRow & IssueListItem);
  }
  return rows;
}

/** The server's count for one day bucket, falling back to the envelope total. */
export function dayTotalFromResponse(response: GroupedIssueListResponse, day: string): number {
  return response.results[day]?.total_results ?? response.total_results;
}

/** Per-day page read: the same window narrowed to one target date. */
export interface CalendarDayQuery {
  day: string;
  perPage: number;
  cursor?: string | undefined;
}

export function calendarDayKey(
  workspaceSlug: string,
  entity: IssuesEntity,
  query: CalendarDayQuery
): readonly unknown[] {
  return [
    ...issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
    "calendar-day",
    query.day,
    query.perPage,
    query.cursor ?? "",
  ];
}

export function calendarDayQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  entity: IssuesEntity,
  query: CalendarDayQuery
) {
  return queryOptions({
    queryKey: calendarDayKey(workspaceSlug, entity, query),
    queryFn: () =>
      readWindow(
        client,
        workspaceSlug,
        projectId,
        entity,
        { after: query.day, before: query.day, perPage: query.perPage },
        query.cursor
      ),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Project states, used only for the block's state-colour edge. */
export function calendarStatesKey(workspaceSlug: string, projectId: string): readonly unknown[] {
  return [...issueKeys.lists(workspaceSlug, projectId), "calendar-states"];
}

export function calendarStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: calendarStatesKey(workspaceSlug, projectId),
    queryFn: () => listStates(client, workspaceSlug, projectId),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
}
