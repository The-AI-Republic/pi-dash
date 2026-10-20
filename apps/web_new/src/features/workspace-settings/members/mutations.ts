// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the members page (parity/specs/workspace-settings.md
// §10–§13; WSET-009/010/012/013). A successful write refreshes the roster
// and the pending list so the page shows what the server stored. A refused
// write keeps the cached rows as they were, so the row that was edited
// stays visible with its previous role (§11–§13). Notifications live here
// because every caller speaks them the same way.
//
// Role writes stay quiet on success — the row's own label is the signal —
// and toast only on failure. Submission and withdrawal always speak.
// Member removal says nothing on success (the row turns suspended) and
// explains a failure with the server's reason when there is one.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  leaveWorkspace,
  removeWorkspaceMember,
  sendWorkspaceInvitations,
  updateWorkspaceInviteRole,
  updateWorkspaceMemberRole,
  withdrawWorkspaceInvite,
  type WorkspaceInvitee,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { rosterKeys } from "./api.js";

/** Pinned notice copy (§10–§13). */
export const INVITATIONS_SENT_TITLE = "Invitations sent";
export const INVITATIONS_SENT_MESSAGE = "Invitations sent. Each invitee will receive their own email.";
export const INVITATION_REMOVED_TITLE = "Success!";
export const INVITATION_REMOVED_MESSAGE = "Invitation removed. That address is no longer pending.";
export const WRITE_FAILED_TITLE = "Error!";

/** Fallbacks when the failure carries no readable reason. */
export const INVITE_FAILED_MESSAGE = "The invitations could not be sent. Check the addresses and try again.";
export const INVITATION_ROLE_FAILED_MESSAGE = "The invitation role could not be changed. Try again.";
export const INVITATION_WITHDRAW_FAILED_MESSAGE = "The invitation could not be removed. Try again.";
export const MEMBER_ROLE_FAILED_MESSAGE = "The member's role could not be changed. Try again.";
export const MEMBER_REMOVE_FAILED_MESSAGE = "The member could not be removed. Try again.";
export const LEAVE_FAILED_MESSAGE = "You could not leave the workspace. Try again.";

/** The reason to show: the failure's own message when it has one. */
export function failureMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return fallback;
}

/** Refresh both lists the roster and the pending group read. */
function useRefreshRoster(workspaceSlug: string): () => Promise<void> {
  const queryClient = useQueryClient();
  return async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: rosterKeys.roster(workspaceSlug) }),
      queryClient.invalidateQueries({ queryKey: rosterKeys.invitations(workspaceSlug) }),
    ]);
  };
}

export interface SendInvitationsInput {
  emails: WorkspaceInvitee[];
}

/** POST .../invitations/ (§10): success closes the dialog; failure keeps it. */
export function useSendInvitations(workspaceSlug: string) {
  const refresh = useRefreshRoster(workspaceSlug);
  return useMutation({
    mutationFn: (input: SendInvitationsInput) =>
      sendWorkspaceInvitations(getAppClient(), workspaceSlug, { emails: input.emails }),
    onSuccess: () => {
      notifySuccess(INVITATIONS_SENT_TITLE, INVITATIONS_SENT_MESSAGE);
    },
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, INVITE_FAILED_MESSAGE));
    },
    onSettled: refresh,
  });
}

export interface InviteRoleInput {
  invitationId: string;
  role: number;
}

/** PATCH .../invitations/{id}/ (§11): a refused write keeps the old role. */
export function useChangeInvitationRole(workspaceSlug: string) {
  const refresh = useRefreshRoster(workspaceSlug);
  return useMutation({
    mutationFn: (input: InviteRoleInput) =>
      updateWorkspaceInviteRole(getAppClient(), workspaceSlug, input.invitationId, { role: input.role }),
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, INVITATION_ROLE_FAILED_MESSAGE));
    },
    onSuccess: refresh,
  });
}

export interface WithdrawInvitationInput {
  invitationId: string;
}

/** DELETE .../invitations/{id}/ (§11): the dialog closes either way. */
export function useWithdrawInvitation(workspaceSlug: string) {
  const refresh = useRefreshRoster(workspaceSlug);
  return useMutation({
    mutationFn: (input: WithdrawInvitationInput) =>
      withdrawWorkspaceInvite(getAppClient(), workspaceSlug, input.invitationId),
    onSuccess: async () => {
      notifySuccess(INVITATION_REMOVED_TITLE, INVITATION_REMOVED_MESSAGE);
      await refresh();
    },
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, INVITATION_WITHDRAW_FAILED_MESSAGE));
    },
  });
}

export interface MemberRoleInput {
  membershipId: string;
  role: number;
}

/** PATCH .../members/{id}/ (§12): quiet success; the label settling is the signal. */
export function useChangeMemberRole(workspaceSlug: string) {
  const refresh = useRefreshRoster(workspaceSlug);
  return useMutation({
    mutationFn: (input: MemberRoleInput) =>
      updateWorkspaceMemberRole(getAppClient(), workspaceSlug, input.membershipId, { role: input.role }),
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, MEMBER_ROLE_FAILED_MESSAGE));
    },
    onSuccess: refresh,
  });
}

export interface RemoveMemberInput {
  membershipId: string;
}

/** DELETE .../members/{id}/ (§13): the row stays listed as suspended. */
export function useRemoveMember(workspaceSlug: string) {
  const refresh = useRefreshRoster(workspaceSlug);
  return useMutation({
    mutationFn: (input: RemoveMemberInput) => removeWorkspaceMember(getAppClient(), workspaceSlug, input.membershipId),
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, MEMBER_REMOVE_FAILED_MESSAGE));
    },
    onSuccess: refresh,
  });
}

/**
 * POST .../members/leave/ (§13). The caller's cached workspace subtree is
 * dropped so no stale row survives, and the surface navigates to the
 * workspace landing. The session's workspace list is left as read: the
 * landing then renders the not-authorized gate for the workspace just left
 * (§13's observed target) instead of a not-found page.
 */
export function useLeaveWorkspace(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      await leaveWorkspace(getAppClient(), workspaceSlug);
      queryClient.removeQueries({ queryKey: ["ws", workspaceSlug] });
    },
    onError: (error: unknown) => {
      notifyError(WRITE_FAILED_TITLE, failureMessage(error, LEAVE_FAILED_MESSAGE));
    },
  });
}
