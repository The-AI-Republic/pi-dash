// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List paging (parity/specs/issues.md §2, ISS-010). The flat list appends
// cursor pages and auto-loads from a sentinel; a grouped list pages each
// section independently, because the server's grouped cursor advances a
// single row-number window across every bucket and its global
// next_page_results flag would otherwise keep asking for a page that
// some small sections never receive. Each section asks for the page that
// starts at its own row count.

import { keepPreviousData, queryOptions, useQueries, useQuery } from "@tanstack/react-query";
import {
  listGroupedCycleIssues,
  listGroupedIssues,
  listGroupedModuleIssues,
  listCycleIssues,
  listIssues,
  listModuleIssues,
  type ApiClient,
  type GroupedIssueListResponse,
  type IssueListResponse,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import { FLAT_PAGE_SIZE, GROUPED_PAGE_SIZE } from "../api/queries.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import {
  flatSection,
  groupedSections,
  mergeGroupedPage,
  sectionHasMore,
  serverGroupBy,
  type IssueGroupSection,
} from "./grouping.js";

export interface ListPageInput {
  entity: IssuesEntity;
  /** Display dimension (state, priority, ...), or null for flat. */
  groupBy: string | null;
  orderBy: string;
  /** Whether child work items join the list (ISS-013/ISS-079). */
  showSubIssues?: boolean;
  cursor?: string;
  /** Stored rich-filter expression (ISS-095–099); absent reads all rows. */
  richFilters?: RichExpression;
}

function projectScoped(entity: IssuesEntity): { projectId: string } | null {
  return "projectId" in entity ? { projectId: entity.projectId } : null;
}

async function readPage(
  client: ApiClient,
  workspaceSlug: string,
  input: ListPageInput
): Promise<IssueListResponse | GroupedIssueListResponse> {
  const scoped = projectScoped(input.entity);
  if (scoped === null) throw new Error("list pages need a project-scoped entity");
  const { projectId } = scoped;
  const serverKey = serverGroupBy(input.groupBy);
  const pageSize = serverKey === null ? FLAT_PAGE_SIZE : GROUPED_PAGE_SIZE;
  const query = withRichFilters(
    {
      perPage: pageSize,
      orderBy: input.orderBy,
      filters: { sub_issue: String(input.showSubIssues === true) },
      ...(input.cursor === undefined ? {} : { cursor: input.cursor }),
    },
    input.richFilters ?? {}
  );
  if (serverKey === null) {
    if (input.entity.scope === "cycle") {
      return listCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, query);
    }
    if (input.entity.scope === "module") {
      return listModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, query);
    }
    return listIssues(client, workspaceSlug, projectId, query);
  }
  const groupedQuery = { ...query, groupBy: serverKey };
  if (input.entity.scope === "cycle") {
    return listGroupedCycleIssues(client, workspaceSlug, projectId, input.entity.cycleId, groupedQuery);
  }
  if (input.entity.scope === "module") {
    return listGroupedModuleIssues(client, workspaceSlug, projectId, input.entity.moduleId, groupedQuery);
  }
  return listGroupedIssues(client, workspaceSlug, projectId, groupedQuery);
}

/** One page read under the entity's list prefix. */
export function listPageQueryOptions(client: ApiClient, workspaceSlug: string, input: ListPageInput) {
  return queryOptions({
    queryKey: [
      ...issueKeys.lists(workspaceSlug, ...entityListsKeyPart(input.entity)),
      "page",
      input.groupBy ?? "flat",
      input.orderBy,
      input.showSubIssues === true,
      input.cursor ?? "first",
      richFiltersParam(input.richFilters ?? {}),
    ] as const,
    queryFn: () => readPage(client, workspaceSlug, input),
    staleTime: LIST_STALE_TIME_MS,
    placeholderData: keepPreviousData,
  });
}

export interface ListPages {
  sections: IssueGroupSection[];
  /** Server total across all buckets/pages. */
  total: number;
  grouped: boolean;
  /** True once any page is loading for the first time. */
  isPending: boolean;
  /** True while another page is in flight. */
  isFetching: boolean;
  hasMore: boolean;
  loadMore: () => void;
  /** Explicit per-section pagination (grouped lists). */
  loadMoreSection: (section: IssueGroupSection) => void;
  /** Whether a specific section still has unseen rows (grouped). */
  sectionMore: (section: IssueGroupSection) => boolean;
}

function totalOf(page: IssueListResponse | GroupedIssueListResponse): number {
  return page.total_count;
}

function isGroupedPage(page: IssueListResponse | GroupedIssueListResponse): page is GroupedIssueListResponse {
  return typeof page.results === "object" && page.results !== null && !Array.isArray(page.results);
}

/** Cursor for the page that starts at a section's current row count. */
export function sectionCursor(rowCount: number, pageSize = GROUPED_PAGE_SIZE): string {
  return `${pageSize}:${Math.floor(rowCount / pageSize)}:0`;
}

/**
 * Page through one entity's list. Flat pages append under one cursor
 * chain; grouped sections each carry their own cursor list so a small
 * bucket's "Load more" fetches that bucket's next window instead of
 * waiting on the global cursor.
 */
export function useListPages(
  workspaceSlug: string,
  entity: IssuesEntity,
  groupBy: string | null,
  orderBy: string,
  labels: ReadonlyMap<string, string>,
  showEmptyGroups: boolean,
  showSubIssues: boolean,
  richFilters: RichExpression = {}
): ListPages {
  const client = getAppClient();
  const serverKey = serverGroupBy(groupBy);
  const grouped = serverKey !== null;
  const resetKey = `${groupBy ?? "flat"}|${orderBy}`;
  const [flatCursors, setFlatCursors] = React.useState<Array<string | undefined>>([undefined]);
  const [sectionCursors, setSectionCursors] = React.useState<Record<string, string[]>>({});
  const [activeKey, setActiveKey] = React.useState(resetKey);
  if (activeKey !== resetKey) {
    setActiveKey(resetKey);
    setFlatCursors([undefined]);
    setSectionCursors({});
  }

  const flatQueries = useQueries({
    queries: grouped
      ? []
      : flatCursors.map((cursor) =>
          listPageQueryOptions(client, workspaceSlug, {
            entity,
            groupBy: null,
            orderBy,
            showSubIssues,
            richFilters,
            ...(cursor === undefined ? {} : { cursor }),
          })
        ),
  });

  const sectionEntries = React.useMemo(
    () => Object.entries(sectionCursors).flatMap(([key, cursors]) => cursors.map((cursor) => ({ key, cursor }))),
    [sectionCursors]
  );
  const sectionQueries = useQueries({
    queries: grouped
      ? sectionEntries.map((entry) =>
          listPageQueryOptions(client, workspaceSlug, {
            entity,
            groupBy,
            orderBy,
            showSubIssues,
            richFilters,
            cursor: entry.cursor,
          })
        )
      : [],
  });

  // The first grouped read always runs, whether or not a section asked
  // for more: it discovers the bucket set.
  const firstGroupedQuery = useQuery({
    ...listPageQueryOptions(client, workspaceSlug, {
      entity,
      groupBy,
      orderBy,
      showSubIssues,
      richFilters,
    }),
    enabled: grouped,
  });

  const flatPages = flatQueries.map((query) => query.data).filter((page) => page !== undefined);
  const firstGroupedPage = firstGroupedQuery.data;

  const sections = React.useMemo(() => {
    if (!grouped) {
      const rows = flatPages.flatMap((page) => (isGroupedPage(page) ? [] : page.results));
      const unique = new Map(rows.map((row) => [row.id, row]));
      const total = flatPages[flatPages.length - 1] === undefined ? 0 : totalOf(flatPages[flatPages.length - 1]!);
      return [flatSection([...unique.values()], total)];
    }
    let merged: IssueGroupSection[] = [];
    if (firstGroupedPage !== undefined && isGroupedPage(firstGroupedPage)) {
      merged = mergeGroupedPage(
        merged,
        groupedSections(firstGroupedPage, labels, { showEmptyGroups, dimension: groupBy })
      );
    }
    for (let index = 0; index < sectionEntries.length; index++) {
      const page = sectionQueries[index]?.data;
      if (page === undefined || !isGroupedPage(page)) continue;
      merged = mergeGroupedPage(merged, groupedSections(page, labels, { showEmptyGroups, dimension: groupBy }));
    }
    return merged;
  }, [grouped, flatPages, firstGroupedPage, sectionEntries, sectionQueries, labels, showEmptyGroups, groupBy]);

  const flatLast = flatPages[flatPages.length - 1];
  const flatHasMore = flatLast?.next_page_results ?? false;
  const total =
    grouped && sections.length > 0
      ? sections.reduce((sum, section) => sum + section.total, 0)
      : flatLast === undefined
        ? 0
        : totalOf(flatLast);

  const loadMore = React.useCallback(() => {
    if (grouped) return;
    const cursor = flatLast?.next_cursor ?? "";
    if (!flatHasMore || cursor === "") return;
    setFlatCursors((current) => (current.includes(cursor) ? current : [...current, cursor]));
  }, [grouped, flatHasMore, flatLast?.next_cursor]);

  const loadMoreSection = React.useCallback((section: IssueGroupSection) => {
    if (!sectionHasMore(section)) return;
    const cursor = sectionCursor(section.rows.length);
    setSectionCursors((current) => {
      const existing = current[section.key] ?? [];
      if (existing.includes(cursor)) return current;
      return { ...current, [section.key]: [...existing, cursor] };
    });
  }, []);

  const isPending = grouped ? firstGroupedQuery.isPending : (flatQueries[0]?.isPending ?? true);
  const isFetching = flatQueries.some((query) => query.isFetching) || sectionQueries.some((query) => query.isFetching);

  return {
    sections,
    total,
    grouped,
    isPending,
    isFetching,
    hasMore: grouped ? false : flatHasMore,
    loadMore,
    loadMoreSection,
    sectionMore: (section) => sectionHasMore(section),
  };
}
