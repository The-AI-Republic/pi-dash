// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views list reads (parity/specs/views.md §2, §9). The screens subscribe
// with useQuery and render a skeleton while the first fetch resolves; a
// route never blocks navigation on this read. Focus and reconnect never
// refetch (shared client default, VIEW-043): the list refreshes on the
// acting user's own writes only.

import { queryOptions } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

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
    // Contracts load with the first read: the project layout route imports
    // this module eagerly, so a static contract import would put the whole
    // views schema set on the initial chain (NEWFRONT-871, size gate).
    queryFn: async () => {
      const { listProjectViews } = await import("@pidash/api-client");
      return listProjectViews(client, workspaceSlug, projectId);
    },
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
    queryFn: async () => {
      const { listWorkspaceViews } = await import("@pidash/api-client");
      return listWorkspaceViews(client, workspaceSlug, { orderBy: "-created_at" });
    },
    staleTime: LIST_STALE_TIME_MS,
  });
}
