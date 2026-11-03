// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero reads (parity/specs/cycles.md §15 and §23,
// CYC-041). The hero names the cycle covering today through the
// only current-scoped list read, then progress, analytics, and the
// first issues page behind the priority tab. Every read keys under
// the shared cycle keys so writes refresh the hero with the rest.

import { queryOptions } from "@tanstack/react-query";
import {
  getCycleAnalytics,
  getCycleProgress,
  listCycleIssues,
  listCycleRows,
  type ApiClient,
  type CycleAnalyticsType,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { cycleKeys } from "./keys.js";

/** First-page width behind the priority tab; the tab filters client-side. */
export const HERO_ISSUES_PAGE_SIZE = 100;

/** The cycle covering today, if any; the server answers a bare array. */
export function heroQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: cycleKeys.hero(workspaceSlug, projectId),
    queryFn: () => listCycleRows(client, workspaceSlug, projectId, { cycleView: "current" }),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Per-group counts behind the progress card and the pending line. */
export function heroProgressQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return queryOptions({
    queryKey: cycleKeys.progress(workspaceSlug, projectId, cycleId),
    queryFn: () => getCycleProgress(client, workspaceSlug, projectId, cycleId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Distribution and chart series behind the burn-down and breakdown cards. */
export function heroAnalyticsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  type: CycleAnalyticsType
) {
  return queryOptions({
    queryKey: cycleKeys.analytics(workspaceSlug, projectId, cycleId, type),
    queryFn: () => getCycleAnalytics(client, workspaceSlug, projectId, cycleId, type),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** First issues page; the priority tab keeps urgent and high rows. */
export function heroIssuesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return queryOptions({
    queryKey: cycleKeys.heroIssues(workspaceSlug, projectId, cycleId),
    queryFn: () => listCycleIssues(client, workspaceSlug, projectId, cycleId, { perPage: HERO_ISSUES_PAGE_SIZE }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
