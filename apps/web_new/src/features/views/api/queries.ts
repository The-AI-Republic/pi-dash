// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views list reads (parity/specs/views.md §2, §9). The screens subscribe
// with useQuery and render a skeleton while the first fetch resolves; a
// route never blocks navigation on this read. Focus and reconnect never
// refetch (shared client default, VIEW-043): the list refreshes on the
// acting user's own writes only.

import { queryOptions } from "@tanstack/react-query";
import { listProjectViews, listWorkspaceViews, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { viewKeys } from "./keys.js";

/**
 * The caller's visible project views: every shared view plus their own
 * (a project guest without `guest_view_all_features` sees only its own
 * rows — the server narrows, the client renders what it gets).
 */
export function projectViewsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: viewKeys.projectList(workspaceSlug, projectId),
    queryFn: () => listProjectViews(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/**
 * The workspace's custom views, newest first (VIEW-029). The four static
 * defaults are client constants and never ride this read; a workspace
 * guest receives only the views they own (server-narrowed).
 */
export function workspaceViewsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: viewKeys.workspaceList(workspaceSlug),
    queryFn: () => listWorkspaceViews(client, workspaceSlug, { orderBy: "-created_at" }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
