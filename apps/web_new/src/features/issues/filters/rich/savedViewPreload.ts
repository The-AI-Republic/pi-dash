// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view preload (parity/specs/issues.md §21, ISS-104). The route module
// is eager (the router imports every route file), so the stored-expression
// and preference modules the screen reads must not be imported from it.
// This module owns them together with the two reads `beforeLoad` awaits:
// the view record and the issue list the view's own stored expression and
// order drive. The screen's chunk loads it in parallel (route.tsx).

import type { ApiClient } from "@pidash/api-client";
import type { QueryClient } from "@tanstack/react-query";

import { issuesQueryOptions, type IssueSearch } from "../../index.js";
import { orderByFromDisplayFilters } from "../../prefs/prefsQuery.js";
import { loadIssueListLeaves, primeIssueListLeaves } from "../../components/lazyLeaves.js";
import { projectViewQueryOptions } from "./savedViewQuery.js";
import { viewExpression } from "./views.js";

/** The route params the saved-view route and its reads take. */
export interface SavedViewReadInput {
  ws: string;
  projectId: string;
  viewId: string;
}

/**
 * Read the view record and then its list, in the order the view stores,
 * and await the list leaves the screen mounts. Errors propagate so the
 * route can decide whether the project gate owns them.
 */
export async function prefetchProjectView(
  queryClient: QueryClient,
  apiClient: ApiClient,
  params: SavedViewReadInput,
  search: IssueSearch
): Promise<void> {
  primeIssueListLeaves();
  const view = await queryClient.ensureQueryData(
    projectViewQueryOptions(apiClient, params.ws, params.projectId, params.viewId)
  );
  await queryClient.ensureQueryData(
    issuesQueryOptions(
      apiClient,
      params.ws,
      params.projectId,
      search,
      viewExpression(view),
      orderByFromDisplayFilters(view.display_filters)
    )
  );
  await loadIssueListLeaves();
}
