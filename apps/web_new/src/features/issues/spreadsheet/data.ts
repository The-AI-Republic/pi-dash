// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spreadsheet data layer (parity/specs/issues.md §3, ISS-015/017/019).
// The sheet reads a flat cursor-paged list (one hundred rows per page)
// ordered by the entity's order_by preference. Writes patch every cached
// page optimistically and revalidate the entity lists on settle; creates
// insert a client temp row and remove it again on failure.

import { infiniteQueryOptions, useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  addIssueToCycle,
  createIssue,
  isApiError,
  listIssues,
  removeIssueFromCycle,
  setIssueModules,
  updateIssue,
  type ApiClient,
  type IssueCreateResult,
  type IssueListItem,
  type IssueListResponse,
  type IssuePatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { createTempId } from "../freshness/fresh.js";
import { issueKeys } from "../api/keys.js";

/** Sheet page size (spec §3: one hundred flat rows). */
export const SHEET_PAGE_SIZE = 100;

/** Failure copy for a refused cell edit. */
export const SHEET_UPDATE_FAILED_TITLE = "Couldn't save the change";
const SHEET_UPDATE_FAILED_BODY = "The cell is unchanged — check your connection and try again.";

export interface SheetQuery {
  orderBy: string;
  perPage?: number;
  /** Flat rows include sub-work items when the display switch is on. */
  showSubIssues?: boolean;
}

/** Infinite sheet pages under the entity's list prefix. */
export function sheetIssuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  query: SheetQuery
) {
  return infiniteQueryOptions({
    queryKey: [
      ...issueKeys.lists(workspaceSlug, projectId),
      "sheet",
      query.orderBy,
      query.perPage ?? SHEET_PAGE_SIZE,
      query.showSubIssues === true,
    ],
    queryFn: ({ pageParam }) =>
      listIssues(client, workspaceSlug, projectId, {
        perPage: query.perPage ?? SHEET_PAGE_SIZE,
        orderBy: query.orderBy,
        filters: { sub_issue: String(query.showSubIssues === true) },
        ...(pageParam === "" ? {} : { cursor: pageParam }),
      }),
    initialPageParam: "",
    getNextPageParam: (lastPage) =>
      lastPage.next_page_results && lastPage.next_cursor !== "" ? lastPage.next_cursor : undefined,
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** All rows across the loaded pages, in server order. */
export function flattenSheetPages(data: { pages: IssueListResponse[] } | undefined): IssueListItem[] {
  if (data === undefined) return [];
  const rows: IssueListItem[] = [];
  for (const page of data.pages) rows.push(...page.results);
  return rows;
}

/** The server total from the first page, or zero before it loads. */
export function sheetTotalCount(data: { pages: IssueListResponse[] } | undefined): number {
  return data?.pages[0]?.total_count ?? 0;
}

interface InfiniteListData {
  pages: IssueListResponse[];
  pageParams: unknown[];
}

function asInfiniteList(value: unknown): InfiniteListData | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const record = value as Partial<InfiniteListData>;
  if (!Array.isArray(record.pages) || !Array.isArray(record.pageParams)) return undefined;
  if (!record.pages.every((page) => Array.isArray(page.results))) return undefined;
  return record as InfiniteListData;
}

/** Map every loaded page of an infinite list cache. */
export function withSheetPages(
  value: unknown,
  update: (page: IssueListResponse, index: number) => IssueListResponse
): unknown {
  const data = asInfiniteList(value);
  if (data === undefined) return value;
  const pages = data.pages.map(update);
  if (pages.every((page, index) => page === data.pages[index])) return value;
  return { ...data, pages };
}

/** Insert a pending row at the head of the first page. */
export function insertSheetTempRow(value: unknown, row: IssueListItem): unknown {
  return withSheetPages(value, (page, index) => {
    if (page.results.some((entry) => entry.id === row.id)) return page;
    return {
      ...page,
      results: index === 0 ? [row, ...page.results] : page.results,
      count: index === 0 ? page.count + 1 : page.count,
      total_count: index === 0 ? page.total_count + 1 : page.total_count,
      total_results: index === 0 ? page.total_results + 1 : page.total_results,
    };
  });
}

/** Drop a pending row from every page. */
export function removeSheetTempRow(value: unknown, tempId: string): unknown {
  return withSheetPages(value, (page) => {
    if (!page.results.some((entry) => entry.id === tempId)) return page;
    return {
      ...page,
      results: page.results.filter((entry) => entry.id !== tempId),
      count: Math.max(0, page.count - 1),
      total_count: Math.max(0, page.total_count - 1),
      total_results: Math.max(0, page.total_results - 1),
    };
  });
}

function withRowFields(row: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  if (typeof row !== "object" || row === null || (row as { id?: unknown }).id !== issueId) return row;
  return { ...row, ...patch };
}

/** Patch one issue's fields across every loaded sheet page. */
export function withSheetIssueFields(value: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  return withSheetPages(value, (page) => ({
    ...page,
    results: page.results.map((row) => withRowFields(row, issueId, patch) as IssueListItem),
  }));
}

/** A synthetic row for an unconfirmed quick-add. */
export function sheetTempIssueRow(tempId: string, projectId: string, name: string): IssueListItem {
  const now = new Date().toISOString();
  return {
    id: tempId,
    name,
    state_id: null,
    sort_order: 0,
    completed_at: null,
    priority: "none",
    sequence_id: 0,
    project_id: projectId,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: now,
    updated_at: now,
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

/** Whether a failure is a server refusal with usable text. */
export function sheetFailureDetail(error: unknown): string {
  return isApiError(error) && error.message !== "" ? error.message : SHEET_UPDATE_FAILED_BODY;
}

/**
 * Optimistically patch all issue caches, run the write, and roll back on
 * failure. Shared by every cell writer so refocus and error handling
 * stay in the cell.
 */
export async function runSheetWrite(
  queryClient: QueryClient,
  workspaceSlug: string,
  issueId: string,
  patch: Record<string, unknown> | object,
  write: () => Promise<void>
): Promise<void> {
  const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
  for (const snapshot of snapshots) {
    if (snapshot.queryKey.includes("sheet")) {
      queryClient.setQueryData(
        snapshot.queryKey,
        withSheetIssueFields(snapshot.data, issueId, { ...patch } as Record<string, unknown>)
      );
    } else {
      queryClient.setQueryData(
        snapshot.queryKey,
        patchElsewhere(snapshot.data, issueId, { ...patch } as Record<string, unknown>)
      );
    }
  }
  try {
    await write();
  } catch (error) {
    rollbackSnapshots(queryClient, snapshots);
    notifyError(SHEET_UPDATE_FAILED_TITLE, sheetFailureDetail(error));
    throw error;
  }
}

/** Patch non-sheet shapes (detail and older flat list caches). */
export function patchElsewhere(value: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  if (Array.isArray(value)) return value.map((row) => withRowFields(row, issueId, patch));
  if (typeof value !== "object" || value === null) return value;
  const record = value as Record<string, unknown>;
  if (typeof record["id"] === "string") return withRowFields(record, issueId, patch);
  if (Array.isArray(record["results"])) {
    return { ...record, results: record["results"].map((row) => withRowFields(row, issueId, patch)) };
  }
  return value;
}

/** Single-field-cell write: PATCH the issue and revalidate its lists. */
export function useSheetFieldUpdate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { issueId: string; patch: IssuePatch }>({
    mutationFn: async ({ issueId, patch }) => {
      await runSheetWrite(queryClient, workspaceSlug, issueId, patch, async () => {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, patch);
      });
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}

/** Cycle-cell write through the cycle membership bridge. */
export function useSheetCycleUpdate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { issueId: string; cycleId: string | null; previousCycleId: string | null }>({
    mutationFn: async ({ issueId, cycleId, previousCycleId }) => {
      await runSheetWrite(queryClient, workspaceSlug, issueId, { cycle_id: cycleId }, async () => {
        if (cycleId === null) {
          if (previousCycleId !== null) {
            await removeIssueFromCycle(getAppClient(), workspaceSlug, projectId, previousCycleId, issueId);
          }
          return;
        }
        await addIssueToCycle(getAppClient(), workspaceSlug, projectId, cycleId, issueId);
      });
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}

/** Module-cell write through the module membership bridge. */
export function useSheetModuleUpdate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { issueId: string; moduleIds: string[]; previousModuleIds: string[] }>({
    mutationFn: async ({ issueId, moduleIds, previousModuleIds }) => {
      const added = moduleIds.filter((id) => !previousModuleIds.includes(id));
      const removed = previousModuleIds.filter((id) => !moduleIds.includes(id));
      await runSheetWrite(queryClient, workspaceSlug, issueId, { module_ids: moduleIds }, async () => {
        await setIssueModules(getAppClient(), workspaceSlug, projectId, issueId, {
          modules: added,
          ...(removed.length === 0 ? {} : { removed_modules: removed }),
        });
      });
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}

/** Success toast title after an inline quick-add lands. */
export const SHEET_CREATED_TITLE = "Work item created";
const SHEET_CREATED_BODY = "It is at the top of the spreadsheet.";

/**
 * Inline quick-add: insert a temp row into the first loaded page, post,
 * replace the temp row with the server row, then revalidate the lists.
 */
export function useSheetCreateIssue(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const listsPrefix = issueKeys.lists(workspaceSlug, projectId);
  return useMutation<IssueCreateResult, Error, { name: string }>({
    mutationFn: async ({ name }) => {
      const tempId = createTempId();
      const tempRow = sheetTempIssueRow(tempId, projectId, name);
      for (const query of queryClient.getQueryCache().findAll({ queryKey: listsPrefix })) {
        const data = queryClient.getQueryData(query.queryKey);
        if (asInfiniteList(data) === undefined) continue;
        queryClient.setQueryData(query.queryKey, insertSheetTempRow(data, tempRow));
      }
      try {
        return await createIssue(getAppClient(), workspaceSlug, projectId, { name });
      } catch (error) {
        for (const query of queryClient.getQueryCache().findAll({ queryKey: listsPrefix })) {
          const data = queryClient.getQueryData(query.queryKey);
          if (asInfiniteList(data) === undefined) continue;
          queryClient.setQueryData(query.queryKey, removeSheetTempRow(data, tempId));
        }
        throw error;
      }
    },
    onSuccess: (created) => {
      for (const query of queryClient.getQueryCache().findAll({ queryKey: listsPrefix })) {
        const data = queryClient.getQueryData(query.queryKey);
        if (asInfiniteList(data) === undefined) continue;
        queryClient.setQueryData(query.queryKey, replaceTempWithCreated(data, created));
      }
      notifySuccess(SHEET_CREATED_TITLE, SHEET_CREATED_BODY);
    },
    onError: (error) => {
      notifyError(SHEET_UPDATE_FAILED_TITLE, sheetFailureDetail(error));
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}

/** Swap the oldest temp row for the confirmed server row. */
export function replaceTempWithCreated(value: unknown, created: IssueCreateResult): unknown {
  const { deleted_at: deletedAt, ...row } = created;
  void deletedAt;
  return withSheetPages(value, (page) => {
    const index = page.results.findIndex((entry) => entry.id.startsWith("temp-"));
    if (index < 0) return page;
    const results = [...page.results];
    results[index] = row;
    return { ...page, results };
  });
}
