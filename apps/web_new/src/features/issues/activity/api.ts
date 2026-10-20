// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Property-change half of the activity feed (parity/specs/issues.md §16,
// ISS-194). Reads come from the issue history with the property activity
// type; comments keep their own query so the two sources merge on screen
// rather than in one server response. Every write to the issue or to a
// comment invalidates this read, which is how a rename or a new comment
// joins the feed without a reload.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listIssueActivities, type ApiClient, type IssueActivity } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";

export interface ActivityScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** The issue's property activities in the order the server returns them. */
export function activitiesQueryOptions(client: ApiClient, scope: ActivityScope) {
  return queryOptions({
    queryKey: issueKeys.activity(scope.workspaceSlug, scope.projectId, scope.issueId),
    queryFn: (): Promise<IssueActivity[]> =>
      listIssueActivities(client, scope.workspaceSlug, scope.projectId, scope.issueId),
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useIssueActivities(scope: ActivityScope) {
  return useQuery(activitiesQueryOptions(getAppClient(), scope));
}
