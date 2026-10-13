// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth queries. The card drives the steps; components read server data
// through these hooks, never raw client calls.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getMyProjectRoles,
  getMyWorkspaceMembership,
  getProject,
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  type ApiClient,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";
import { meQueryOptions } from "../../core/session/queries.js";

export const authKeys = {
  invitation: (slug: string, invitationId: string) => ["auth", "invitation", slug, invitationId] as const,
  membership: (slug: string) => [...workspaceScope(slug), "auth", "membership"] as const,
  myInvitations: ["auth", "my-invitations"] as const,
  myJoinRequests: ["auth", "my-join-requests"] as const,
  project: (slug: string, projectId: string) => [...workspaceScope(slug), "auth", "project", projectId] as const,
  projectRoles: (slug: string) => [...workspaceScope(slug), "auth", "project-roles"] as const,
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

/**
 * The caller's own membership in one workspace (AUTH-024). Null-pattern
 * reads report null, which splits Case A from Case B. Fetched only once
 * the slug is known in the workspace list.
 */
export function membershipQueryOptions(client: ApiClient, slug: string) {
  return queryOptions({
    queryKey: authKeys.membership(slug),
    queryFn: () => getMyWorkspaceMembership(client, slug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The caller's per-project roles in one workspace (the permission-store feed). */
export function projectRolesQueryOptions(client: ApiClient, slug: string) {
  return queryOptions({
    queryKey: authKeys.projectRoles(slug),
    queryFn: () => getMyProjectRoles(client, slug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/**
 * One project row (AUTH-024). The project gate reads this for its fetch
 * status (409/403/404) rather than for the row itself.
 */
export function projectQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return queryOptions({
    queryKey: authKeys.project(slug, projectId),
    queryFn: () => getProject(client, slug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The caller's own membership in one workspace; null when they hold none. */
export function useWorkspaceMembership(slug: string) {
  return useQuery(membershipQueryOptions(getAppClient(), slug));
}

/** The caller's per-project roles in one workspace, keyed by project id. */
export function useProjectRoles(slug: string) {
  return useQuery(projectRolesQueryOptions(getAppClient(), slug));
}

/** One project row with its fetch status for the denial screens. */
export function useProjectRow(slug: string, projectId: string) {
  return useQuery(projectQueryOptions(getAppClient(), slug, projectId));
}

/** The signed-in address, or undefined for visitors and failed reads. */
export function useViewerEmail(): string | undefined {
  const { data } = useQuery(meQueryOptions(getAppClient()));
  return data?.email;
}
