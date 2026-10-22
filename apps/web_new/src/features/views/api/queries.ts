// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views list reads (parity/specs/views.md §2, VIEW-001). The screen
// subscribes with useQuery and renders a skeleton while the first fetch
// resolves; the route never blocks navigation on this read. Focus and
// reconnect never refetch (shared client default, VIEW-043): the list
// refreshes on the acting user's own writes only.

import { queryOptions } from "@tanstack/react-query";
import { listProjectViews, type ApiClient } from "@pidash/api-client";

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
