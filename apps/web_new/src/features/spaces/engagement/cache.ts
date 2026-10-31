// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public engagement cache writes (parity/specs/space.md §3.8, SPC-047/048).
// The overlay reads the issue detail, the cards read the grouped list
// pages; an optimistic toggle patches every cached copy of the issue so
// both surfaces move together, and a failed write restores the snapshot
// before the reconcile refetch lands.

import type { QueryClient, QueryKey } from "@tanstack/react-query";
import type { SpaceBoardIssue, SpaceBoardIssueDetail, SpaceBoardIssuePage } from "@pidash/api-client";

import { spaceKeys } from "../api/keys.js";

/** Rewrite one cached issue row; rows that do not match stay untouched. */
export type SpaceIssuePatch = (issue: SpaceBoardIssue) => SpaceBoardIssue;

function patchPage(page: SpaceBoardIssuePage, issueId: string, patch: SpaceIssuePatch): SpaceBoardIssuePage {
  let touched = false;
  const results: Record<string, SpaceBoardIssuePage["results"][string]> = {};
  for (const [stateId, bucket] of Object.entries(page.results)) {
    const rows = bucket.results.map((issue) => {
      if (issue.id !== issueId) return issue;
      touched = true;
      return patch(issue);
    });
    results[stateId] = touched ? { results: rows, total_results: bucket.total_results } : bucket;
  }
  return touched ? { ...page, results } : page;
}

/**
 * Every cached list page of one board: the first pages under each filter
 * signature plus any later group pages the columns fetched. Detail keys
 * are matched separately, never through this predicate.
 */
export function isSpaceBoardPageKey(anchor: string, key: QueryKey): boolean {
  return (
    Array.isArray(key) &&
    key.length >= 5 &&
    key[0] === "spaces" &&
    key[1] === "anchor" &&
    key[2] === anchor &&
    key[3] === "issues"
  );
}

export interface SpaceEngagementSnapshot {
  detailKey: readonly unknown[];
  detail: SpaceBoardIssueDetail | undefined;
  pages: Array<{ key: QueryKey; page: SpaceBoardIssuePage | undefined }>;
}

/** Snapshot every cached copy of one issue for a later rollback. */
export function snapshotSpaceEngagement(
  queryClient: QueryClient,
  anchor: string,
  issueId: string
): SpaceEngagementSnapshot {
  const detailKey = spaceKeys.detail(anchor, issueId);
  const pages = queryClient
    .getQueryCache()
    .findAll({ predicate: (query) => isSpaceBoardPageKey(anchor, query.queryKey) })
    .map((query) => ({ key: query.queryKey, page: queryClient.getQueryData<SpaceBoardIssuePage>(query.queryKey) }));
  return { detailKey, detail: queryClient.getQueryData<SpaceBoardIssueDetail>(detailKey), pages };
}

/** Patch every cached copy of one issue (detail plus list pages). */
export function applySpaceEngagement(
  queryClient: QueryClient,
  anchor: string,
  issueId: string,
  patch: SpaceIssuePatch
): void {
  // The key carries the issue id, so a cached detail here is this issue's.
  queryClient.setQueryData<SpaceBoardIssueDetail | undefined>(spaceKeys.detail(anchor, issueId), (detail) =>
    detail === undefined ? detail : { ...detail, ...patch(detail) }
  );
  for (const query of queryClient
    .getQueryCache()
    .findAll({ predicate: (query) => isSpaceBoardPageKey(anchor, query.queryKey) })) {
    queryClient.setQueryData<SpaceBoardIssuePage | undefined>(query.queryKey, (page) =>
      page === undefined ? page : patchPage(page, issueId, patch)
    );
  }
}

/** Roll one issue's cached copies back to a snapshot after a failed write. */
export function restoreSpaceEngagement(queryClient: QueryClient, snapshot: SpaceEngagementSnapshot): void {
  queryClient.setQueryData(snapshot.detailKey, snapshot.detail);
  for (const { key, page } of snapshot.pages) {
    queryClient.setQueryData(key, page);
  }
}

/**
 * Re-read one issue's stored engagement after a write settles (SPC-047/048):
 * the detail plus every cached list page of the board.
 */
export async function invalidateSpaceEngagement(
  queryClient: QueryClient,
  anchor: string,
  issueId: string
): Promise<void> {
  await queryClient.invalidateQueries({ queryKey: spaceKeys.detail(anchor, issueId) });
  await queryClient.invalidateQueries({ predicate: (query) => isSpaceBoardPageKey(anchor, query.queryKey) });
}
