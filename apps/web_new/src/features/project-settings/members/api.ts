// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab reads (parity/specs/project-settings-members.md §"Member
// table"; PSET-018). Two reads resolve on entry: the project's memberships
// (id, user id, role, joining time) and the workspace directory that turns
// user ids into names, avatars and emails. The page leaves its skeleton
// only when both settle. Failure titles and the server detail are shaped
// here so the page and the toasts agree.

import { queryOptions, useMutation, useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import * as React from "react";
import {
  addProjectMembers,
  isApiError,
  leaveProject,
  listMembers,
  listWorkspaceMembers,
  removeProjectMember,
  updateProject,
  updateProjectMember,
  type ApiClient,
  type ProjectDetails,
  type ProjectDetail,
  type ProjectMemberAddInput,
  type ProjectMemberDetail,
  type ProjectMemberList,
  type ProjectMemberRoleList,
  type ProjectUpdateInput,
  type WorkspaceMemberList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { authKeys, useProjectRoles, useWorkspaceMembership } from "../../auth/index.js";
import { useMe } from "../../../core/session/index.js";
import { projectDetailsQueryOptions, projectKeys } from "../../projects/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

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

// ---------------------------------------------------------------------------
// Writes (parity/specs/project-settings-members.md §"Add members",
// §"Inline role changes", §"Remove and leave-self", §"Project defaults";
// PSET-022–026). Every mutation folds its answer into the caches the tab and
// the project shell read, so the surface settles without a refetch.

/** Confirmation titles the write paths raise on success. */
export const MEMBERS_ADDED_TITLE = "Members added";
export const DEFAULTS_SAVED_TITLE = "Project defaults saved";
/** Failure titles the write paths raise. */
export const UPDATE_ROLE_FAILED_TITLE = "Couldn't change the role";
export const REMOVE_MEMBER_FAILED_TITLE = "Couldn't remove the member";
export const LEAVE_PROJECT_FAILED_TITLE = "Couldn't leave the project";
export const SAVE_DEFAULTS_FAILED_TITLE = "Couldn't save the project defaults";
/** Fallback detail a refused write shows when the server sent none. */
export const WRITE_FAILED_FALLBACK = "Check your connection and try again.";

function writeFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return WRITE_FAILED_FALLBACK;
}

export interface ProjectDefaultsFields {
  project_lead: string | null;
  default_assignee: string | null;
  guest_view_all_features: boolean;
}

/**
 * The defaults the section edits, read from the workspace's rich project
 * rows (the detail read carries the default assignee; the list row does
 * not, and widening that row would grow the eager shell chunk). Undefined
 * until the read answers, so the section shows its loaders.
 */
export function useProjectDefaults(workspaceSlug: string, projectId: string): UseQueryResult<ProjectDefaultsFields> {
  const client = getAppClient();
  const rows = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  return React.useMemo(
    () => ({
      ...rows,
      data: rows.data?.find((row) => row.id === projectId) as ProjectDefaultsFields | undefined,
    }),
    [rows, projectId]
  ) as UseQueryResult<ProjectDefaultsFields>;
}

export interface AddMembersInput {
  members: ProjectMemberAddInput[];
}

/**
 * Bulk-add every row in one POST. Success confirms and refreshes the
 * memberships and the workspace directory (for identities the tab had not
 * seen yet); a failed submit stays silent apart from a console entry —
 * no failure notice renders (PSET-022).
 */
export function useAddProjectMembers(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectMemberRoleList, Error, AddMembersInput>({
    mutationFn: ({ members }) => addProjectMembers(getAppClient(), workspaceSlug, projectId, members),
    onSuccess: () => {
      notifySuccess(MEMBERS_ADDED_TITLE);
      void queryClient.invalidateQueries({ queryKey: memberKeys.memberships(workspaceSlug, projectId) });
      void queryClient.invalidateQueries({ queryKey: memberKeys.directory(workspaceSlug) });
    },
    onError: (error) => {
      console.error(error);
    },
  });
}

export interface MemberRoleInput {
  memberId: string;
  role: number;
}

/**
 * PATCH one membership's role. The caller applies the change optimistically
 * (the table writes the new word before the round-trip and rolls back via
 * `onError` here); a refused write toasts the server message and leaves the
 * table standing (PSET-023/PSET-027).
 */
export function useUpdateProjectMemberRole(
  workspaceSlug: string,
  projectId: string,
  onRollback?: (input: MemberRoleInput) => void
) {
  const queryClient = useQueryClient();
  return useMutation<ProjectMemberDetail, Error, MemberRoleInput>({
    mutationFn: ({ memberId, role }) => updateProjectMember(getAppClient(), workspaceSlug, projectId, memberId, role),
    onError: (error, input) => {
      onRollback?.(input);
      notifyError(UPDATE_ROLE_FAILED_TITLE, writeFailureDetail(error));
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: memberKeys.memberships(workspaceSlug, projectId) });
    },
  });
}

/** DELETE one membership (soft deactivation); the row leaves the table. */
export function useRemoveProjectMember(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<number, Error, { memberId: string }>({
    mutationFn: ({ memberId }) => removeProjectMember(getAppClient(), workspaceSlug, projectId, memberId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: memberKeys.memberships(workspaceSlug, projectId) });
    },
    onError: (error) => {
      notifyError(REMOVE_MEMBER_FAILED_TITLE, writeFailureDetail(error));
    },
  });
}

/**
 * POST the leave endpoint for the caller's own membership. Success means
 * the caller no longer belongs to the project, so the project feeds and the
 * routes that gate on them are dropped rather than refreshed.
 */
export function useLeaveProject(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<number, Error, void>({
    mutationFn: () => leaveProject(getAppClient(), workspaceSlug, projectId),
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: authKeys.project(workspaceSlug, projectId) });
      queryClient.removeQueries({ queryKey: memberKeys.memberships(workspaceSlug, projectId) });
      void queryClient.invalidateQueries({ queryKey: authKeys.projectRoles(workspaceSlug) });
    },
    onError: (error) => {
      notifyError(LEAVE_PROJECT_FAILED_TITLE, writeFailureDetail(error));
    },
  });
}

/** One defaults field the section writes. */
export type ProjectDefaultsPatch = Pick<
  ProjectUpdateInput,
  "project_lead" | "default_assignee" | "guest_view_all_features"
>;

/**
 * Save one defaults choice through the project update. Success confirms
 * and the answered row stands in for the retrieve cache so the pickers
 * paint the saved value; a refusal toasts the title and rolls back
 * nothing the caller did not change itself.
 */
export function useSaveProjectDefaults(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectDetails, Error, ProjectDefaultsPatch>({
    mutationFn: (patch) => updateProject(getAppClient(), workspaceSlug, projectId, patch),
    onSuccess: (project) => {
      notifySuccess(DEFAULTS_SAVED_TITLE);
      queryClient.setQueryData(authKeys.project(workspaceSlug, projectId), (row: ProjectDetail | undefined) =>
        row === undefined ? row : { ...row, ...project }
      );
      queryClient.setQueryData(projectKeys.details(workspaceSlug), (rows: ProjectDetails[] | undefined) =>
        rows?.map((entry) => (entry.id === projectId ? { ...entry, ...project } : entry))
      );
    },
    onError: (error) => {
      notifyError(SAVE_DEFAULTS_FAILED_TITLE, writeFailureDetail(error));
    },
  });
}

/** The signed-in editor's identity, as the role-cell and row-menu rules need it. */
export interface MemberEditorState {
  userId: string | null;
  workspaceRole: number | null;
  projectRole: number | null;
  /** True once the three identity reads have all answered. */
  ready: boolean;
}

/**
 * Resolve the caller's identity for the row rules: their user id, their
 * workspace role and their project role (null outside a membership). The
 * three reads are independent; a failed one leaves its field null, which
 * only ever narrows what the surface offers.
 */
export function useMemberEditor(workspaceSlug: string, projectId: string): MemberEditorState {
  const me = useMe();
  const workspace = useWorkspaceMembership(workspaceSlug);
  const roles = useProjectRoles(workspaceSlug);
  return {
    userId: me.data?.id ?? null,
    workspaceRole: workspace.data?.role ?? null,
    projectRole: roles.data?.[projectId] ?? null,
    ready: !me.isPending && !workspace.isPending && !roles.isPending,
  };
}
