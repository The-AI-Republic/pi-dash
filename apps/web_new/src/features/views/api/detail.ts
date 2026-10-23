// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One project view's detail record (parity/specs/views.md §7, VIEW-021/022).
// The read normalises the server's blank stand-in record to a 404 in the
// contract, so every downstream failure — forbidden, missing, blank or a
// failed fetch — is handled by the screen as the missing-view state.

import { queryOptions } from "@tanstack/react-query";
import { getProjectView, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { viewKeys } from "./keys.js";

/** The detail record, read once per view and never live-refreshed. */
export function projectViewDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  viewId: string
) {
  return queryOptions({
    queryKey: viewKeys.projectDetail(workspaceSlug, projectId, viewId),
    queryFn: () => getProjectView(client, workspaceSlug, projectId, viewId),
    staleTime: LIST_STALE_TIME_MS,
    // A failed view read is a terminal missing-view state for this visit;
    // retrying a 403 or a missing id only delays the honest answer.
    retry: false,
  });
}
