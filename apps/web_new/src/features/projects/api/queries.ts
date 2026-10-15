// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project queries. Route loaders prefetch these; components read them
// through hooks, never through the HTTP client directly.

import { queryOptions } from "@tanstack/react-query";
import { listProjectDetails, listProjects, type ApiClient } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { projectKeys } from "./keys.js";

export function projectsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: projectKeys.list(workspaceSlug),
    queryFn: () => listProjects(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/**
 * The workspace collection the projects list renders (SHELL-024). One
 * query per workspace, shared by the list and archives routes, so the
 * collection is fetched once and both views read the same rows. Screens
 * subscribe with useQuery and render skeletons while it resolves — the
 * routes never block navigation on this read (SHELL-025).
 */
export function projectDetailsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: projectKeys.details(workspaceSlug),
    queryFn: () => listProjectDetails(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}
