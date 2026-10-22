// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Property-change half of the activity feed (parity/specs/issues.md §16,
// ISS-194; incremental pickup in parity/specs/comments.md §4.5, CMT-017).
// Reads come from the issue history with the property activity type;
// comments keep their own query so the two sources merge on screen
// rather than in one server response. The first read fetches the whole
// list; every refetch after that carries `created_at__gt` set to the
// newest cached timestamp and joins the delta in place. Every write to
// the issue or to a comment invalidates this read, which is how a rename
// or a new comment joins the feed without a reload.

import { queryOptions, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { listIssueActivities, type ApiClient, type IssueActivity } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { mergeDeltaRows, newestCreatedAt } from "../api/incremental.js";
import { issueKeys } from "../api/keys.js";

export interface ActivityScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/**
 * The issue's property activities in the order the server returns them.
 * With the query client the refetch is incremental: it reads the newest
 * cached timestamp as its cursor and merges the delta. Without it (or on
 * an empty cache) the read fetches the whole list.
 */
export function activitiesQueryOptions(client: ApiClient, scope: ActivityScope, queryClient?: QueryClient) {
  const key = issueKeys.activity(scope.workspaceSlug, scope.projectId, scope.issueId);
  return queryOptions({
    queryKey: key,
    queryFn: async (): Promise<IssueActivity[]> => {
      const cached = queryClient?.getQueryData<IssueActivity[]>(key);
      const cursor = cached === undefined ? undefined : newestCreatedAt(cached);
      if (cached === undefined || cursor === undefined) {
        return listIssueActivities(client, scope.workspaceSlug, scope.projectId, scope.issueId);
      }
      const delta = await listIssueActivities(client, scope.workspaceSlug, scope.projectId, scope.issueId, {
        createdAfter: cursor,
      });
      return mergeDeltaRows(cached, delta);
    },
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useIssueActivities(scope: ActivityScope) {
  const queryClient = useQueryClient();
  return useQuery(activitiesQueryOptions(getAppClient(), scope, queryClient));
}
