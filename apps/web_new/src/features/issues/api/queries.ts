// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue queries (read-only in this slice). Route loaders prefetch the
// list with ensureQueryData; the screen reads it back with useSuspenseQuery.

import { queryOptions } from "@tanstack/react-query";
import { listIssues, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import type { IssueSearch } from "../filters/search.js";
import { issueKeys } from "./keys.js";

export function issuesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, search: IssueSearch) {
  return queryOptions({
    queryKey: issueKeys.list(workspaceSlug, projectId, search),
    queryFn: () => listIssues(client, workspaceSlug, projectId, { perPage: 50, orderBy: "-created_at" }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
