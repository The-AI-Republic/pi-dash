// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace detail preload (parity/specs/views.md §11, VIEW-038/040).
// Warms the reads and chunks the screen needs so the committed route
// paints settled: the customs list (the access check), the custom record
// when the id is visible to the caller, the scoped first issue page, and
// the empty-card and filter chunks. Every miss settles into the screen's
// own states (missing, error, skeleton); data never rejects navigation.

import type { ApiClient } from "@pidash/api-client";
import type { QueryClient } from "@tanstack/react-query";

import {
  globalIssuesQueryOptions,
  loadIssueFilters,
  loadRichFilterBuilder,
  loadSavedViewEmptyCard,
  orderByFromDisplayFilters,
  withRichFilters,
} from "../../issues/index.js";
import { workspaceViewDetailQueryOptions } from "../api/detail.js";
import { workspaceViewsQueryOptions } from "../api/queries.js";
import { findVisibleCustomView, findWorkspaceDefaultView } from "./workspaceDetail.js";

/** Route params the workspace detail screen and its reads take. */
export interface WorkspaceViewDetailParams {
  ws: string;
  viewId: string;
}

/**
 * Read the customs list, then the record and the scoped first page it
 * selects, and start the chunks the screen mounts. Customs, record and
 * list failures settle quietly; the screen renders their states from
 * the settled cache.
 */
export async function preloadWorkspaceView(
  queryClient: QueryClient,
  apiClient: ApiClient,
  params: WorkspaceViewDetailParams
): Promise<void> {
  const card = loadSavedViewEmptyCard();
  const customs = await queryClient.ensureQueryData(workspaceViewsQueryOptions(apiClient, params.ws)).catch(() => null);
  // The empty card is needed only when a body read yields no rows; wait
  // for it with the list so the first commit can show either.
  await card.catch(() => undefined);
  if (customs === null) return;
  const defaultView = findWorkspaceDefaultView(params.viewId);
  if (defaultView !== null) {
    if (defaultView.key === "all-issues") {
      await queryClient
        .ensureInfiniteQueryData(globalIssuesQueryOptions(apiClient, params.ws, {}))
        .catch(() => undefined);
    }
    // Member-scoped defaults need the session user for their static
    // condition; the screen fetches their first page on mount.
    await loadRichFilterBuilder().catch(() => undefined);
    return;
  }
  // An id the visible list hides never earns its retrieve: the fetch
  // alone is not guest-narrowed, so the screen answers missing.
  if (findVisibleCustomView(customs, params.viewId) === null) return;
  const view = await queryClient
    .ensureQueryData(workspaceViewDetailQueryOptions(apiClient, params.ws, params.viewId))
    .catch(() => null);
  if (view === null) return;
  await queryClient
    .ensureInfiniteQueryData(
      globalIssuesQueryOptions(
        apiClient,
        params.ws,
        withRichFilters({ orderBy: orderByFromDisplayFilters(view.display_filters) }, view.rich_filters)
      )
    )
    .catch(() => undefined);
  await loadIssueFilters().catch(() => undefined);
}
