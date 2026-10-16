// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The client-side sign-out sequence (AUTH-017). Every entry point — the
// shell menus and palette, the switch and deactivation dialogs, and later
// the not-found screen and the change-email dialog — runs this one path:
// a fresh-CSRF sign-out POST, then client clearing, then the desktop local
// teardown, then the signed-out landing. Written from
// parity/specs/auth.md; there is no agent runtime to dispose in this app,
// and on desktop the daemon lifecycle rides the teardown seam the desktop
// epic owns (DESK-010).

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { signOut as postSignOut, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { platform, type SessionTeardownApi } from "../../core/platform/index.js";
import { signOut as clearSession } from "../../core/session/store.js";
import { notifyError } from "../../shared/shell/index.js";

export const SIGN_OUT_FAILED_TITLE = "Could not sign out";
// The failure message is pinned by the shell oracle (SHELL-058): the
// workspace-switcher scenario asserts this exact text, so the wording
// stays aligned with the old app.
export const SIGN_OUT_FAILED_MESSAGE = "Failed to sign out. Please try again.";

/** The signed-out entry: the sign-in card at home, the login route on desktop. */
export function signOutLandingTarget(kind: "web" | "desktop"): string {
  return kind === "desktop" ? "/login" : "/";
}

/**
 * Run the sign-out sequence against the given client and cache. A failed
 * POST toasts and rethrows so the caller leaves the user where they are;
 * a failed local teardown is swallowed because the server session is
 * already gone and landing on sign-in stays right. Resolves to the
 * signed-out landing target.
 */
export async function performSignOut(args: {
  client: ApiClient;
  queryClient: QueryClient;
  teardown?: SessionTeardownApi | undefined;
}): Promise<string> {
  const { client, queryClient, teardown } = args;
  client.clearCsrfCache();
  try {
    await postSignOut(client);
  } catch (error) {
    notifyError(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    throw error;
  }
  clearSession(queryClient);
  if (teardown !== undefined) {
    try {
      await teardown.clearLocalSession();
    } catch {
      // The server session is already gone; a failed local wipe must not
      // strand the user on a dead session.
    }
  }
  return signOutLandingTarget(platform.kind);
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
