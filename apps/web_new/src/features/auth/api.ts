// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Auth mutations. Thin wrappers over the api-client contracts: the card
// drives the steps, these hooks only talk to the server and keep the
// session store in sync. Sign-in completion navigates through the server
// landing URL (see SignInCard), so success needs no cache work here.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  checkEmail,
  generateMagicCode,
  signIn,
  signInWithMagicCode,
  signOut as postSignOut,
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

export function usePasswordSignIn() {
  return useMutation({
    mutationFn: (input: { email: string; password: string; nextPath?: string }) =>
      signIn(getAppClient(), input).then(markSignedIn),
  });
}

export function useMagicSignIn() {
  return useMutation({
    mutationFn: (input: { email: string; code: string; nextPath?: string }) =>
      signInWithMagicCode(getAppClient(), input).then(markSignedIn),
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
