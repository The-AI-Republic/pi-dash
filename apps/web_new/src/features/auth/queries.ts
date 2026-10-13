// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth queries. The card drives the steps; components read server data
// through these hooks, never raw client calls.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  type ApiClient,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";

export const authKeys = {
  invitation: (slug: string, invitationId: string) => ["auth", "invitation", slug, invitationId] as const,
  myInvitations: ["auth", "my-invitations"] as const,
  myJoinRequests: ["auth", "my-join-requests"] as const,
};

/** One emailed invitation for the invitation-aware card header (AUTH-005). */
export function invitationQueryOptions(client: ApiClient, slug: string, invitationId: string) {
  return queryOptions({
    queryKey: authKeys.invitation(slug, invitationId),
    queryFn: () => getWorkspaceInvitation(client, slug, invitationId),
    enabled: slug.length > 0 && invitationId.length > 0,
  });
}

/** The emailed invitation when the card URL carries one; disabled otherwise. */
export function useWorkspaceInvitation(slug?: string | undefined, invitationId?: string | undefined) {
  return useQuery(invitationQueryOptions(getAppClient(), slug ?? "", invitationId ?? ""));
}

/** The caller's own workspace invitations (AUTH-026 inbox, AUTH-029 progress). Reusable across the area. */
export function myInvitationsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: authKeys.myInvitations,
    queryFn: () => listWorkspaceInvitations(client),
  });
}

/** The caller's own workspace invitations. */
export function useMyWorkspaceInvitations() {
  return useQuery(myInvitationsQueryOptions(getAppClient()));
}

/** The caller's own workspace join requests (AUTH-034 holding view). Reusable across the area. */
export function myJoinRequestsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: authKeys.myJoinRequests,
    queryFn: () => listWorkspaceJoinRequests(client),
  });
}

/** The caller's own workspace join requests. */
export function useMyWorkspaceJoinRequests() {
  return useQuery(myJoinRequestsQueryOptions(getAppClient()));
}
