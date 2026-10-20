// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab reads (parity/specs/project-settings-members.md §"Member
// table"; PSET-018). Two reads resolve on entry: the project's memberships
// (id, user id, role, joining time) and the workspace directory that turns
// user ids into names, avatars and emails. The page leaves its skeleton
// only when both settle. Failure titles and the server detail are shaped
// here so the page and the toasts agree.

import { queryOptions, useQuery, type UseQueryResult } from "@tanstack/react-query";
import {
  isApiError,
  listMembers,
  listWorkspaceMembers,
  type ApiClient,
  type ProjectMemberList,
  type WorkspaceMemberList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { notifyError } from "../../../shared/shell/index.js";

/** Failure title when the two roster reads cannot load. */
export const MEMBERS_LOAD_FAILED_TITLE = "Couldn't load the members";
/** Fallback detail when a failure carries no server text. */
export const MEMBERS_LOAD_FAILED_FALLBACK = "Check your connection and try again.";

export const memberKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "project-settings", "members"] as const,
  memberships: (workspaceSlug: string, projectId: string) =>
    [...memberKeys.all(workspaceSlug), "memberships", projectId] as const,
  directory: (workspaceSlug: string) => [...memberKeys.all(workspaceSlug), "directory"] as const,
};

/** The project's memberships; reference data with the shared freshness. */
export function projectMembersQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions<ProjectMemberList>({
    queryKey: memberKeys.memberships(workspaceSlug, projectId),
    queryFn: () => listMembers(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The workspace directory: identity facts for every membership row. */
export function workspaceDirectoryQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceMemberList>({
    queryKey: memberKeys.directory(workspaceSlug),
    queryFn: () => listWorkspaceMembers(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useProjectMemberships(workspaceSlug: string, projectId: string): UseQueryResult<ProjectMemberList> {
  return useQuery(projectMembersQueryOptions(getAppClient(), workspaceSlug, projectId));
}

export function useWorkspaceDirectory(workspaceSlug: string): UseQueryResult<WorkspaceMemberList> {
  return useQuery(workspaceDirectoryQueryOptions(getAppClient(), workspaceSlug));
}

/** Server detail for a read failure, falling back when it carried none. */
export function memberFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return MEMBERS_LOAD_FAILED_FALLBACK;
}

/**
 * Toast a failed roster read once. The surface keeps whatever it already
 * shows (skeleton or table), matching PSET-027: failures never clear the
 * table.
 */
export function notifyMembersLoadFailure(error: unknown): void {
  notifyError(MEMBERS_LOAD_FAILED_TITLE, memberFailureDetail(error));
}
