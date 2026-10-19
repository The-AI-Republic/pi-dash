// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view reads for the issues list (parity/specs/issues.md §21,
// ISS-104). A list rendered from a saved view loads the view record so
// its stored expression drives the list and the filter row; create and
// update stay in ViewActions. Full view management belongs to the views
// area, so this is the minimal read the issues gate needs.

import { queryOptions } from "@tanstack/react-query";
import { getProjectView, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../../core/query/client.js";
import { savedViewKeys } from "./views.js";

/** One project view record, read once per view and kept fresh on refetch. */
export function projectViewQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, viewId: string) {
  return queryOptions({
    queryKey: savedViewKeys.project(workspaceSlug, projectId, viewId),
    queryFn: () => getProjectView(client, workspaceSlug, projectId, viewId),
    staleTime: LIST_STALE_TIME_MS,
  });
}
