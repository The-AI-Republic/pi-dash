// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view detail preload (parity/specs/views.md §7, VIEW-021/022/028).
// The route file is eager, so the reads and the chunks that render their
// result live here and the route requests them on visit. The order matters:
// the view record first, because its stored expression and order shape the
// issue read; then that list; then the layout chunk the record selects.
//
// A failed view read is not a load error — it is the missing-view state
// (VIEW-022). So the view read and the issue read it depends on settle
// quietly and the screen renders the empty state from the settled cache.
// Only the layout chunk's own failure is left to throw.

import type { ApiClient } from "@pidash/api-client";
import type { QueryClient } from "@tanstack/react-query";

import {
  loadIssueListLeaves,
  loadSavedViewEmptyCard,
  issuesQueryOptions,
  orderByFromDisplayFilters,
  preloadViewLayout,
  primeIssueListLeaves,
  type IssueSearch,
} from "../../issues/index.js";
import { projectViewDetailQueryOptions } from "../api/detail.js";
import { savedViewLayout } from "./model.js";

/** Route params the detail screen and its reads take. */
export interface SavedViewDetailParams {
  ws: string;
  projectId: string;
  viewId: string;
}

/**
 * Read the view record, then the list its stored expression and order
 * select, and start the chunks the screen mounts. View and list failures
 * settle into the missing-view state; they never reject the navigation.
 */
export async function preloadSavedView(
  queryClient: QueryClient,
  apiClient: ApiClient,
  params: SavedViewDetailParams,
  search: IssueSearch
): Promise<void> {
  primeIssueListLeaves();
  const card = loadSavedViewEmptyCard();
  const view = await queryClient
    .ensureQueryData(projectViewDetailQueryOptions(apiClient, params.ws, params.projectId, params.viewId))
    .catch(() => null);
  // The empty card is needed only when a read or the stored query yields no
  // rows; wait for it with the list so the first commit can show either.
  await card.catch(() => undefined);
  if (view === null) return;
  await queryClient
    .ensureQueryData(
      issuesQueryOptions(
        apiClient,
        params.ws,
        params.projectId,
        search,
        view.rich_filters,
        orderByFromDisplayFilters(view.display_filters)
      )
    )
    .catch(() => undefined);
  await Promise.all([
    loadIssueListLeaves().catch(() => undefined),
    preloadViewLayout(savedViewLayout(view)).catch(() => undefined),
  ]);
}
