// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile reads for the member pages (parity/specs/profile.md §1–4): the
// identity/per-project segregation, the assigned-work statistics, one
// ownership-selected issue list, and the paged activity feed. Export is a
// write, so it lives in ./export.ts.

import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import {
  getUserProfileActivity,
  getUserProfileSegregation,
  getUserProfileStats,
  listUserProfileIssues,
  type ApiClient,
  type ProfileOwnership,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS, REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { profileKeys } from "./keys.js";

/** Rows fetched per issue-tab page; the tab follows cursors past this. */
export const PROFILE_ISSUES_PAGE_SIZE = 100;

/** Activity rows per page in the full activity tab (§3). */
export const PROFILE_ACTIVITY_PAGE_SIZE = 100;

/** Recent activity rows on the summary (§1). */
export const PROFILE_OVERVIEW_ACTIVITY_SIZE = 10;

/** Identity, avatar, timezone and the per-project counts. */
export function profileSegregationQueryOptions(client: ApiClient, workspaceSlug: string, userId: string) {
  return queryOptions({
    queryKey: profileKeys.segregation(workspaceSlug, userId),
    queryFn: () => getUserProfileSegregation(client, workspaceSlug, userId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Count cards, workload and the two distributions. */
export function profileStatsQueryOptions(client: ApiClient, workspaceSlug: string, userId: string) {
  return queryOptions({
    queryKey: profileKeys.stats(workspaceSlug, userId),
    queryFn: () => getUserProfileStats(client, workspaceSlug, userId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/**
 * One ownership-selected issue tab. The tab reads the first page and every
 * later page through the server cursor, so a member with more rows than
 * one page still sees the whole set.
 */
export function profileIssuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  userId: string,
  ownership: ProfileOwnership,
  orderBy: string
) {
  return infiniteQueryOptions({
    queryKey: [...profileKeys.issues(workspaceSlug, userId, ownership), orderBy, "pages"],
    queryFn: ({ pageParam }) =>
      listUserProfileIssues(client, workspaceSlug, userId, {
        ownership,
        perPage: PROFILE_ISSUES_PAGE_SIZE,
        orderBy,
        ...(pageParam === "" ? {} : { cursor: pageParam }),
      }),
    initialPageParam: "",
    getNextPageParam: (last) => (last.next_page_results ? last.next_cursor : undefined),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The first activity page at the given size. */
export function profileActivityQueryOptions(client: ApiClient, workspaceSlug: string, userId: string, perPage: number) {
  return queryOptions({
    queryKey: profileKeys.activity(workspaceSlug, userId, perPage),
    queryFn: () => getUserProfileActivity(client, workspaceSlug, userId, { perPage }),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}
