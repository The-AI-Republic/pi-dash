// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth queries. The card drives the steps; components read server data
// through these hooks, never raw client calls.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getMyProjectMembership,
  getMyProjectRoles,
  getMyWorkspaceMembership,
  getProject,
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
  projectMembership: (slug: string, projectId: string) =>
    [...workspaceScope(slug), "auth", "project-membership", projectId] as const,
};

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
 * The caller's own membership row in one project, read inline. The
 * notification detail pane resolves triage versus ordinary content from
 * this lookup, so it stays separate from the warmed per-workspace role
 * map: each selection really reaches the server and its own fetch status
 * drives the pane's loading state (parity/specs/notifications.md §5).
 */
export function projectMembershipQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return queryOptions({
    queryKey: authKeys.projectMembership(slug, projectId),
    queryFn: () => getMyProjectMembership(client, slug, projectId),
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
