// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One request's detail read (parity/specs/intake.md INT-019/031, Data
// layer). A one-shot fetch under the intake detail key with the shared
// no-refetch policy; the selection is the key's last leg, so moving
// between requests reads each one once. The embedded work item omits
// the count/cycle/module keys the work-item viewset annotates, so the
// adapter below restores them before the shared detail renders it.

import { useQuery } from "@tanstack/react-query";
import { getIntakeIssue, isApiError, type ApiClient, type IntakeIssueDetailItem, type Issue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { intakeKeys } from "./keys.js";
import { INTAKE_QUERY_POLICY } from "./queries.js";

/** Query options for one request's detail, shaped for `useQuery`. */
export function intakeDetailQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return {
    queryKey: intakeKeys.detail(workspaceSlug, projectId, issueId),
    queryFn: () => getIntakeIssue(client, workspaceSlug, projectId, issueId),
    ...INTAKE_QUERY_POLICY,
  };
}

/** One request's detail for the pane. */
export function useIntakeDetail(workspaceSlug: string, projectId: string, issueId: string) {
  return useQuery(intakeDetailQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
}

/**
 * The detail's embedded work item as the shared `Issue`: the keys the
 * intake viewset never annotates read as their empty values.
 */
export function intakeDetailWorkItem(item: IntakeIssueDetailItem): Issue {
  return {
    ...item,
    cycle_id: item.cycle_id ?? null,
    module_ids: item.module_ids ?? [],
    sub_issues_count: item.sub_issues_count ?? null,
    attachment_count: item.attachment_count ?? null,
    link_count: item.link_count ?? null,
    is_subscribed: item.is_subscribed ?? false,
  };
}

/** True when the detail read failed because the id is stale or forbidden (INT-028 step 3). */
export function isMissingIntakeDetail(error: unknown): boolean {
  return isApiError(error) && (error.status === 404 || error.status === 403);
}
