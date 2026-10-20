// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue reads (parity/specs/issues.md §15 + §22 ISS-174/238). One
// cursor-paged query per parent, every page asked for at the fifty-row
// cap; the next page exists only while the envelope carries a cursor, so
// a small parent resolves in one request and shows no load-more control.
// The first page also carries the full-set distribution, total and the
// rollup, which is why the title always reads the newest envelope.

import { queryOptions, useInfiniteQuery, useQuery } from "@tanstack/react-query";
import {
  isApiError,
  listEpicWorkItems,
  listSubIssues,
  searchIssues,
  type ApiClient,
  type IssueSearchResults,
  type SubIssueItem,
  type SubIssuesResponse,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import { nextSubIssueCursor, SUB_ISSUE_PAGE_SIZE } from "./rules.js";

/**
 * One parent's cache key. Epic children share the key with issue
 * children: one parent never changes kind, so the key stays one entry.
 */
export function subIssuesKey(workspaceSlug: string, projectId: string, parentId: string) {
  return [...issueKeys.all(workspaceSlug), "sub-issues", projectId, parentId] as const;
}

/**
 * The children read for one parent kind (ISS-174). Epics serve their
 * children under `epics/<id>/issues/`, issues under
 * `issues/<id>/sub-issues/`; both answer the same page shape.
 */
export function listChildren(
  client: ApiClient,
  scope: { workspaceSlug: string; projectId: string; parentId: string; isEpic: boolean },
  query: { perPage?: number; cursor?: string } = {}
): Promise<SubIssuesResponse> {
  if (scope.isEpic) {
    return listEpicWorkItems(client, scope.workspaceSlug, scope.projectId, scope.parentId, query);
  }
  return listSubIssues(client, scope.workspaceSlug, scope.projectId, scope.parentId, query);
}

/** One parent's page position; the first page carries no cursor. */
export interface SubIssuePageParam {
  cursor: string | undefined;
}

/** The paged read behind the widget. */
export function subIssuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  parentId: string,
  isEpic = false
) {
  return {
    queryKey: subIssuesKey(workspaceSlug, projectId, parentId),
    queryFn: ({ pageParam }: { pageParam: SubIssuePageParam }): Promise<SubIssuesResponse> =>
      listChildren(
        client,
        { workspaceSlug, projectId, parentId, isEpic },
        {
          perPage: SUB_ISSUE_PAGE_SIZE,
          ...(pageParam.cursor === undefined ? {} : { cursor: pageParam.cursor }),
        }
      ),
    initialPageParam: { cursor: undefined } as SubIssuePageParam,
    getNextPageParam: (last: SubIssuesResponse): SubIssuePageParam | undefined => {
      const cursor = nextSubIssueCursor(last);
      return cursor === null ? undefined : { cursor };
    },
  };
}

/**
 * The query hook a parent's section calls. The read runs whether or not
 * the section is open: the collapsed title still needs the counts, and a
 * remounted parent must render its header before anyone can reopen it.
 */
export function useSubIssues(
  workspaceSlug: string,
  projectId: string,
  parentId: string,
  options: { isEpic?: boolean } = {}
) {
  return useInfiniteQuery({
    ...subIssuesQueryOptions(getAppClient(), workspaceSlug, projectId, parentId, options.isEpic ?? false),
  });
}

/** Every loaded row of a parent, page order preserved. */
export function loadedRows(cache: { pages: readonly SubIssuesResponse[] } | undefined): SubIssueItem[] {
  if (cache === undefined) return [];
  return cache.pages.flatMap((page) => page.sub_issues);
}

/** The envelope the title reads: the first page carries the full-set rollup. */
export function latestEnvelope(
  cache: { pages: readonly SubIssuesResponse[] } | undefined
): SubIssuesResponse | undefined {
  return cache?.pages[0];
}

/** Everything a sub-issue write needs: one parent under its project. */
export interface SubIssueScope {
  workspaceSlug: string;
  projectId: string;
  parentId: string;
  /** Epic parents attach through their own children collection. */
  isEpic: boolean;
}

/** Server detail for a failure notice, falling back when empty. */
export function failureDetail(error: unknown, fallback: string): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return fallback;
}

/** Cache key for the add-existing search under one project. */
export function subIssueSearchKey(workspaceSlug: string, projectId: string, text: string) {
  return [...issueKeys.all(workspaceSlug), "sub-issues", "search", projectId, text] as const;
}

/**
 * The add-existing candidates (ISS-175): the project-scoped issue search
 * with the parent exclusion on, so the parent and its children stay out.
 * No workspace toggle: the endpoint is project-scoped by design.
 */
export function subIssueSearchQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, text: string) {
  return queryOptions({
    queryKey: subIssueSearchKey(workspaceSlug, projectId, text),
    queryFn: (): Promise<IssueSearchResults> =>
      searchIssues(client, workspaceSlug, projectId, { search: text, subIssue: true }),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled: projectId !== "",
  });
}

/** Add-existing candidates; the caller debounces the term. */
export function useSubIssueSearch(workspaceSlug: string, projectId: string, text: string, enabled: boolean) {
  return useQuery({
    ...subIssueSearchQueryOptions(getAppClient(), workspaceSlug, projectId, text),
    enabled: enabled && projectId !== "",
  });
}
