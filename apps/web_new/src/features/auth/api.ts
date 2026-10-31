// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth mutations. Thin wrappers over the api-client contracts: the card
// drives the steps, these hooks only talk to the server and keep the
// session store in sync. Sign-in completion navigates through the server
// landing URL (see SignInCard), so success needs no cache work here.
// Every credential submission drops the cached forgery token first so the
// POST carries a freshly fetched one (AUTH-020).

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  acceptWorkspaceInvitations,
  answerWorkspaceInvitation,
  approveDeviceCode,
  checkEmail,
  checkWorkspaceSlug,
  completeTour,
  createWorkspace,
  createWorkspaceJoinRequest,
  deactivateAccount,
  generateMagicCode,
  getMeSettings,
  getProfile,
  joinProjects,
  listWorkspaces,
  markOnboarded,
  requestPasswordReset,
  resetPassword,
  sendWorkspaceInvitations,
  setPassword,
  signIn,
  signInWithMagicCode,
  signUp,
  signUpWithMagicCode,
  updateMe,
  updateProfile,
  uploadUserAvatar,
  type CreatedWorkspace,
  type JoinRequestResult,
  type MePatch,
  type MeSettings,
  type Profile,
  type ResetPasswordInput,
  type SetPasswordResult,
  type SignInResult,
  type WorkspaceCreateInput,
  type WorkspaceInvitee,
  type WorkspaceList,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { platform } from "../../core/platform/index.js";
import { invalidateWorkspace } from "../../core/query/optimistic.js";
import { resolveLanding } from "../../core/session/guards.js";
import { sessionKeys } from "../../core/session/queries.js";
import { useSessionStore } from "../../core/session/store.js";
import { markInitialColorScheme, notifyError, notifySuccess } from "../../shared/shell/index.js";
// NOTE (NEWFRONT-294, size gate): finishProfileStamp stays behind a dynamic
// import so the onboarding flow module never joins the initial bundle — the
// finish mutation only runs from the onboarding route's gesture.
async function loadFinishProfileStamp(): Promise<typeof import("./onboarding.js").finishProfileStamp> {
  return (await import("./onboarding.js")).finishProfileStamp;
}
import { authKeys } from "./queries.js";
import { performSignOut } from "./signOut.js";

export function useEmailCheck() {
  return useMutation({
    mutationFn: (email: string) => checkEmail(getAppClient(), email),
  });
}

export function useMagicGenerate() {
  return useMutation({
    mutationFn: (email: string) => generateMagicCode(getAppClient(), email),
  });
}

function markSignedIn(result: SignInResult): SignInResult {
  if (result.ok) {
    useSessionStore.getState().markAuthenticated();
  }
  return result;
}

/**
 * The app client with a dropped forgery-token cache: the next unsafe
 * request fetches a fresh token, so every credential submission carries
 * one issued for this submit (AUTH-020).
 */
function freshClient() {
  const client = getAppClient();
  client.clearCsrfCache();
  return client;
}

export function usePasswordSignIn() {
  return useMutation({
    mutationFn: (input: { email: string; password: string; nextPath?: string }) =>
      signIn(freshClient(), input).then(markSignedIn),
  });
}

export function useMagicSignIn() {
  return useMutation({
    mutationFn: (input: { email: string; code: string; nextPath?: string }) =>
      signInWithMagicCode(freshClient(), input).then(markSignedIn),
  });
}

export function usePasswordSignUp() {
  return useMutation({
    mutationFn: (input: { email: string; password: string; confirmPassword: string; nextPath?: string }) =>
      signUp(freshClient(), input).then(markSignedIn),
  });
}

export function useMagicSignUp() {
  return useMutation({
    mutationFn: (input: { email: string; code: string; nextPath?: string }) =>
      signUpWithMagicCode(freshClient(), input).then(markSignedIn),
  });
}

/** Forgot-password request (AUTH-012). Success mails the reset link. */
export function useForgotPassword() {
  return useMutation({
    mutationFn: (email: string) => requestPasswordReset(freshClient(), email),
  });
}

/** Password reset from the emailed uid/token link (AUTH-013). */
export function usePasswordReset() {
  return useMutation({
    mutationFn: (input: ResetPasswordInput) => resetPassword(freshClient(), input),
  });
}

/** First password for passwordless accounts (AUTH-014). */
export function useSetPassword() {
  return useMutation({
    mutationFn: (password: string): Promise<SetPasswordResult> => setPassword(freshClient(), password),
  });
}

/** Display-name and avatar writes (AUTH-031). Refreshes the cached user row. */
export function useUpdateMe() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: MePatch) => updateMe(getAppClient(), input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

/** Avatar image upload (AUTH-031). Resolves to the public URL to store on the user row. */
export function useUploadUserAvatar() {
  return useMutation({
    mutationFn: (file: File) => uploadUserAvatar(getAppClient(), file),
  });
}

/** Slug availability read (AUTH-035). Resolves true when the slug is free. */
export function useCheckWorkspaceSlug() {
  return useMutation({
    mutationFn: async (slug: string): Promise<boolean> => {
      const answer = await checkWorkspaceSlug(getAppClient(), slug);
      return answer.status;
    },
  });
}

/**
 * Workspace creation (AUTH-034): post the workspace, stamp it as the last
 * workspace, then refresh the cached workspace list and settings so the
 * finish landing validates against them. Resolves to the created row.
 */
export function useCreateWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: WorkspaceCreateInput): Promise<CreatedWorkspace> => {
      const client = getAppClient();
      const created = await createWorkspace(client, input);
      await updateProfile(client, { last_workspace_id: created.id });
      const [workspaces, settings] = await Promise.all([listWorkspaces(client), getMeSettings(client)]);
      queryClient.setQueryData<WorkspaceList>(sessionKeys.workspaces, workspaces);
      queryClient.setQueryData<MeSettings>(sessionKeys.settings, settings);
      return created;
    },
  });
}

/**
 * Batch-accept invitations (AUTH-034): join the selected workspaces and
 * refresh the cached list and settings. The caller finishes onboarding.
 */
export function useAcceptWorkspaceInvitations() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (invitationIds: string[]): Promise<void> => {
      const client = getAppClient();
      await acceptWorkspaceInvitations(client, invitationIds);
      const [workspaces, settings] = await Promise.all([listWorkspaces(client), getMeSettings(client)]);
      queryClient.setQueryData<WorkspaceList>(sessionKeys.workspaces, workspaces);
      queryClient.setQueryData<MeSettings>(sessionKeys.settings, settings);
    },
  });
}

/**
 * Workspace access request (AUTH-036): post the admin address. When the
 * answer names a workspace slug the caller already belongs there, so the
 * hook refreshes the cached list and settings before the caller finishes.
 * Always refreshes the caller's own join-request list.
 */
export function useCreateJoinRequest() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (adminEmail: string): Promise<JoinRequestResult> => {
      const client = getAppClient();
      const result = await createWorkspaceJoinRequest(client, { admin_email: adminEmail });
      if (result.workspace_slug !== undefined) {
        const [workspaces, settings] = await Promise.all([listWorkspaces(client), getMeSettings(client)]);
        queryClient.setQueryData<WorkspaceList>(sessionKeys.workspaces, workspaces);
        queryClient.setQueryData<MeSettings>(sessionKeys.settings, settings);
      }
      await queryClient.invalidateQueries({ queryKey: authKeys.myJoinRequests });
      return result;
    },
  });
}

/**
 * Bulk workspace invites (AUTH-037): post the filled rows against the
 * given workspace slug. Resolves to the server message.
 */
export function useSendWorkspaceInvites() {
  return useMutation({
    mutationFn: async (input: { slug: string; emails: WorkspaceInvitee[] }): Promise<string> => {
      const result = await sendWorkspaceInvitations(getAppClient(), input.slug, { emails: input.emails });
      return result.message;
    },
  });
}

export const TOUR_COMPLETE_FAILED_TITLE = "Could not save the tour";

/**
 * Complete the first-run product tour (AUTH-042): flip the profile flag,
 * then stamp the cached profile so the overlay closes at once. A failed
 * write toasts the server detail and rejects, leaving the overlay up so
 * the user can retry the dismissal.
 */
export function useCompleteTour() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (): Promise<void> => {
      try {
        await completeTour(getAppClient(), { is_tour_completed: true });
      } catch (error) {
        notifyError(TOUR_COMPLETE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
      queryClient.setQueryData<Profile>(sessionKeys.profile, (current) =>
        current === undefined ? current : { ...current, is_tour_completed: true }
      );
    },
  });
}

export const FINISH_FAILURE_TITLE = "Could not finish onboarding";

/**
 * Finish onboarding (AUTH-039, shared with S8): stamp every progress flag
 * plus the last-workspace pointer, mark the profile onboarded, refresh the
 * cached profile and settings, then mark the local profile onboarded.
 * Resolves to the AUTH-016 landing target. Callers refresh the workspace
 * list first (creation/join steps do), so landing validates against it. A
 * failed finish toasts and rejects, leaving the user on the step.
 */
export function useFinishOnboarding() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (): Promise<string> => {
      const client = getAppClient();
      const workspaces = queryClient.getQueryData<WorkspaceList>(sessionKeys.workspaces);
      try {
        await updateProfile(client, (await loadFinishProfileStamp())(workspaces));
        await markOnboarded(client, { is_onboarded: true });
        const [profile, settings] = await Promise.all([getProfile(client), getMeSettings(client)]);
        queryClient.setQueryData<Profile>(sessionKeys.profile, { ...profile, is_onboarded: true });
        queryClient.setQueryData<MeSettings>(sessionKeys.settings, settings);
        return resolveLanding({
          lastSlug: settings.workspace.last_workspace_slug,
          fallbackSlug: settings.workspace.fallback_workspace_slug,
          workspaces,
        });
      } catch (error) {
        notifyError(FINISH_FAILURE_TITLE, (error as Error).message);
        throw error;
      }
    },
  });
}

/**
 * CLI device approval (AUTH-021). Posts the typed code with a fresh
 * forgery token; resolves with the stamped account and workspace.
 */
export function useApproveDeviceCode() {
  return useMutation({
    mutationFn: (userCode: string) => approveDeviceCode(freshClient(), userCode),
  });
}

export const JOIN_PROJECT_FAILED_TITLE = "Could not join the project";

/**
 * Join a project from the denial screen (AUTH-024): post the join with a
 * fresh forgery token, then refresh the workspace subtree (the permission
 * feeds plus the project's entities) so the gate renders the project. A
 * failed join toasts the server detail and rejects, keeping the screen.
 */
export function useJoinProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: { slug: string; projectId: string }) => {
      try {
        const result = await joinProjects(freshClient(), input.slug, [input.projectId]);
        await invalidateWorkspace(queryClient, input.slug);
        return result;
      } catch (error) {
        notifyError(JOIN_PROJECT_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
  });
}

/**
 * Switch accounts (AUTH-018): sign out per AUTH-017, reset the theme to
 * the system default, and land on the signed-out entry for a fresh login.
 * A failed sign-out toasts and rejects; the dialog stays open.
 */
export function useSwitchAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      const landing = await performSignOut({
        client: getAppClient(),
        queryClient,
        teardown: platform.sessionTeardown,
        skipRuntimeConfirm: true,
      });
      markInitialColorScheme();
      return landing;
    },
  });
}

export const DEACTIVATE_SUCCESS_TITLE = "Account deactivated";
export const DEACTIVATE_SUCCESS_MESSAGE = "You have been signed out.";
export const DEACTIVATE_FAILED_TITLE = "Could not deactivate your account";

/**
 * Deactivate the account (AUTH-019): disable it, toast the outcome, then
 * sign out per AUTH-017 and return to the signed-out entry. A failed
 * deactivation toasts the server detail and rejects, leaving the account
 * signed in.
 */
export function useDeactivateAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      const client = getAppClient();
      try {
        await deactivateAccount(client);
      } catch (error) {
        // The client throws ApiError for every failure, so the message is
        // always the server detail.
        notifyError(DEACTIVATE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
      notifySuccess(DEACTIVATE_SUCCESS_TITLE, DEACTIVATE_SUCCESS_MESSAGE);
      return performSignOut({ client, queryClient, teardown: platform.sessionTeardown, skipRuntimeConfirm: true });
    },
  });
}

export const SINGLE_ANSWER_FAILED_TITLE = "Could not answer the invitation";

export interface AnswerInvitationInput {
  slug: string;
  invitationId: string;
  accepted: boolean;
  token: string;
}

/**
 * Answer one emailed invitation (AUTH-027): accept or ignore it through
 * the join URL. The caller lands per AUTH-028; a failed answer toasts the
 * server detail and rejects, leaving the pending card on screen.
 */
export function useAnswerWorkspaceInvitation() {
  return useMutation({
    mutationFn: async ({ slug, invitationId, accepted, token }: AnswerInvitationInput) => {
      try {
        return await answerWorkspaceInvitation(getAppClient(), slug, invitationId, { accepted, token });
      } catch (error) {
        // The client throws ApiError for every failure, so the message is
        // always the server detail.
        notifyError(SINGLE_ANSWER_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
  });
}
