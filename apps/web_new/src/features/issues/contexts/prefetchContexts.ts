// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First reads for the archived route (parity/specs/issues.md §8, ISS-071).
// That route definition lives in the eager route graph, so the query
// options and their feature modules ride this module's chunk; only this
// body is imported dynamically from the route (Quality gates,
// NEWFRONT-637). The workspace all-issues route (ISS-073) mounts the
// list host (NEWFRONT-656), whose suspense reads need no prefetch.

import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { projectsQueryOptions } from "../../projects/index.js";

export interface ContextPrefetchInput {
  queryClient: QueryClient;
  apiClient: ApiClient;
  workspaceSlug: string;
  projectId?: string;
}

/** Fill the project collection the archived tab resolves its project from. */
export async function prefetchArchivedIssues({
  queryClient,
  apiClient,
  workspaceSlug,
}: ContextPrefetchInput): Promise<void> {
  await queryClient.ensureQueryData(projectsQueryOptions(apiClient, workspaceSlug));
}
