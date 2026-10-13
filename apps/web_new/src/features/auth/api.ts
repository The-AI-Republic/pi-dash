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
  checkEmail,
  deactivateAccount,
  generateMagicCode,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signInWithMagicCode,
  signUp,
  signUpWithMagicCode,
  type ResetPasswordInput,
  type SetPasswordResult,
  type SignInResult,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { platform } from "../../core/platform/index.js";
import { useSessionStore } from "../../core/session/store.js";
import { markInitialColorScheme, notifyError, notifySuccess } from "../../shared/shell/index.js";
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

/**
 * Sign out everywhere (AUTH-017): end the server session, then clear every
 * client store and the query cache, then the desktop local teardown.
 * Resolves to the signed-out landing target; a failed POST toasts and
 * rejects instead, so the caller leaves the user where they are.
 */
export function useSignOut() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => performSignOut({ client: getAppClient(), queryClient, teardown: platform.sessionTeardown }),
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
      return performSignOut({ client, queryClient, teardown: platform.sessionTeardown });
    },
  });
}
