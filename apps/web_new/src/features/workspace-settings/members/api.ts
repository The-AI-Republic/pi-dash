// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members roster reads (parity/specs/workspace-settings.md §7–§9,
// WSET-008/014). The roster resolves two reads on entry — the member
// directory and the pending invitations — and the page waits for both
// before leaving its skeleton. Settings lists read on mount only (§38).

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  listPendingWorkspaceInvites,
  listWorkspaceRoster,
  type ApiClient,
  type WorkspaceInvitationList,
  type WorkspaceRosterMemberList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../lib/settingsQuery.js";

export const rosterKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "workspace-settings", "members"] as const,
  roster: (workspaceSlug: string) => [...rosterKeys.all(workspaceSlug), "roster"] as const,
  invitations: (workspaceSlug: string) => [...rosterKeys.all(workspaceSlug), "invitations"] as const,
};

/** The member directory through the admin lens (suspended rows included). */
export function rosterQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceRosterMemberList>({
    queryKey: rosterKeys.roster(workspaceSlug),
    queryFn: () => listWorkspaceRoster(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
    // The directory changes rarely; keep the reference freshness over the
    // list default. Mount-only reads come from the settings policy.
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/**
 * The pending invitations. This slice renders none of them (the manage
 * half does); the roster reads them because the skeleton and the
 * no-match state wait on both lists (§9).
 */
export function invitationsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceInvitationList>({
    queryKey: rosterKeys.invitations(workspaceSlug),
    queryFn: () => listPendingWorkspaceInvites(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useWorkspaceRoster(workspaceSlug: string) {
  return useQuery(rosterQueryOptions(getAppClient(), workspaceSlug));
}

export function usePendingInvitations(workspaceSlug: string) {
  return useQuery(invitationsQueryOptions(getAppClient(), workspaceSlug));
}
