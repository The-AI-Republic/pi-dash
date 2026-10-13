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
  generateMagicCode,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signInWithMagicCode,
  signOut as postSignOut,
  signUp,
  signUpWithMagicCode,
  type ResetPasswordInput,
  type SetPasswordResult,
  type SignInResult,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { signOut as clearSession, useSessionStore } from "../../core/session/store.js";

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
 * Sign out everywhere: end the server session (best effort — a dead
 * session is already gone), then clear every client store and the query
 * cache. The caller navigates to sign-in afterwards.
 */
export function useSignOut() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      try {
        await postSignOut(getAppClient());
      } catch {
        // The server session is already unusable; local state still clears.
      }
      clearSession(queryClient);
    },
  });
}
