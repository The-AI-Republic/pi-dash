// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-item count (parity/specs/archives.md §1.3, ARCH-002).
// The header badge reads the first archived-issues page's total, not the
// page length, so it stays correct past the first page. Components read
// through the hook, never through the HTTP client directly.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listArchivedIssues, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { archivesKeys } from "./keys.js";

export function archivedCountQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: archivesKeys.count(workspaceSlug, projectId),
    queryFn: async () => (await listArchivedIssues(client, workspaceSlug, projectId)).total_count,
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Total archived work items, for the header badge. */
export function useArchivedIssuesCount(workspaceSlug: string, projectId: string) {
  return useQuery(archivedCountQueryOptions(getAppClient(), workspaceSlug, projectId));
}
