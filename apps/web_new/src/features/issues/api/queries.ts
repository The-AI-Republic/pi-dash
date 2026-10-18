// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue queries. Route loaders prefetch with ensureQueryData; screens
// read back with useSuspenseQuery. The detail hydrate (ISS-142) never
// revalidates on window focus: detail does not live-update (ISS-221).

import { queryOptions } from "@tanstack/react-query";
import {
  getDescriptionVersion,
  getIssue,
  listDescriptionVersions,
  listIssues,
  type ApiClient,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";
import type { IssueSearch } from "../filters/search.js";
import { issueKeys } from "./keys.js";

/** Rows per request for the flat issue list (parity/specs/issues.md §6). */
export const ISSUES_PAGE_SIZE = 100;
/** Sort used until the user picks another order (ISS-002). */
export const DEFAULT_ISSUE_ORDER = "sort_order";

/**
 * The project's flat issue list. Order is the user's stored display
 * preference (manual by default) and rides in the key: the timeline renders
 * rows in server order, so a missing `order_by` would quietly sort them by
 * creation instead (ISS-044/045, ISS-050). Rich filters (ISS-095–099) ride
 * in the key as their serialized form.
 */
export function issuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  search: IssueSearch,
  richExpression: RichExpression = {},
  orderBy: string = DEFAULT_ISSUE_ORDER
) {
  const filters = richFiltersParam(richExpression);
  return queryOptions({
    queryKey: [...issueKeys.list(workspaceSlug, projectId, search, orderBy), filters],
    queryFn: () =>
      listIssues(
        client,
        workspaceSlug,
        projectId,
        withRichFilters({ perPage: ISSUES_PAGE_SIZE, orderBy }, richExpression)
      ),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Full detail hydrate behind a resolved lite row (ISS-142, phase two). */
export function issueDetailQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: issueKeys.detail(workspaceSlug, issueId),
    queryFn: () => getIssue(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** Stored description versions, newest first (ISS-147). */
export function descriptionVersionsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
) {
  return queryOptions({
    queryKey: issueKeys.descriptionVersions(workspaceSlug, projectId, issueId),
    queryFn: () => listDescriptionVersions(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** One stored version with the HTML a restore writes back (ISS-147). */
export function descriptionVersionQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  versionId: string
) {
  return queryOptions({
    queryKey: issueKeys.descriptionVersion(workspaceSlug, projectId, issueId, versionId),
    queryFn: () => getDescriptionVersion(client, workspaceSlug, projectId, issueId, versionId),
    staleTime: LIST_STALE_TIME_MS,
  });
}
