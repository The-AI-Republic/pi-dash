// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The client-side sign-out sequence (AUTH-017). Every entry point — the
// shell menus and palette, the switch and deactivation dialogs, and later
// the not-found screen and the change-email dialog — runs this one path:
// the desktop runtime disposal first (RUN-047: confirm, then daemon stop
// and server enrollment removal while the session is still live), then a
// fresh-CSRF sign-out POST, then client clearing, then the native webview
// wipe, then the signed-out landing. Written from parity/specs/auth.md;
// the runtime steps arrive through the registered hooks the runners
// feature owns, so this file never imports a domain feature.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  cloudSignOut as postCloudSignOut,
  signOut as postSignOut,
  type ApiClient,
  type InstanceConfig,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { markExplicitLogout } from "./cloudLogin.js";
import { platform, type SessionTeardownApi } from "../../core/platform/index.js";
import { getRuntimeSignOutHooks, instanceKeys } from "../../core/session/index.js";
import { signOut as clearSession } from "../../core/session/store.js";
import { notifyError } from "../../shared/shell/index.js";

/** Session storage for the tab marker, or null where the runtime has none. */
function tabSessionStorage(): Pick<Storage, "setItem"> | null {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

export const SIGN_OUT_FAILED_TITLE = "Could not sign out";
// The failure message is pinned by the shell oracle (SHELL-058): the
// workspace-switcher scenario asserts this exact text, so the wording
// stays aligned with the old app.
export const SIGN_OUT_FAILED_MESSAGE = "Failed to sign out. Please try again.";

/** Cloud sign-out landing (CLOUD-016): the login card, reached by a full navigation. */
export const CLOUD_SIGN_OUT_LANDING = "/login";

/**
 * True when the cached instance config says this is the hosted (cloud)
 * backend (CLOUD-016, from parity/specs/cloud.md §4). An unread config
 * reads self-managed, keeping today's OSS sequence.
 */
export function isHostedBackend(queryClient: QueryClient): boolean {
  return queryClient.getQueryData<InstanceConfig>(instanceKeys.config)?.is_self_managed === false;
}

/** The signed-out entry: the sign-in card at home, the login route on desktop. */
export function signOutLandingTarget(kind: "web" | "desktop"): string {
  return kind === "desktop" ? "/login" : "/";
}

export interface PerformSignOutArgs {
  client: ApiClient;
  queryClient: QueryClient;
  teardown?: SessionTeardownApi | undefined;
  /**
   * Skip the desktop runtime confirmation (for sign-outs the user already
   * confirmed: account switch, deactivation, email change). The runtime is
   * still disposed, keeping chat history.
   */
  skipRuntimeConfirm?: boolean | undefined;
}

/**
 * Run the sign-out sequence against the given client and cache. A failed
 * POST toasts and rethrows so the caller leaves the user where they are;
 * a failed local teardown is swallowed because the server session is
 * already gone and landing on sign-in stays right. Resolves to the
 * signed-out landing target, or null when the user cancels the desktop
 * runtime confirmation (nothing is signed out then).
 *
 * When runtime hooks are registered (desktop), the runtime disposal runs
 * before the POST: server enrollment removal needs a live session. The
 * disposal itself is best-effort, so a failed revocation never traps the
 * user; a failed POST afterwards still leaves the session intact.
 *
 * On the hosted backend (CLOUD-016) the POST is the cloud logout pair and
 * the landing is a full replacing navigation to /login, so every store
 * and cache starts from scratch; the upstream provider session is left
 * alone. Everywhere else the OSS sequence below runs unchanged.
 */
export function performSignOut(args: PerformSignOutArgs & { skipRuntimeConfirm: true }): Promise<string>;
export function performSignOut(
  args: PerformSignOutArgs & { skipRuntimeConfirm?: false | undefined }
): Promise<string | null>;
export async function performSignOut(args: PerformSignOutArgs): Promise<string | null> {
  const { client, queryClient, teardown } = args;
  const runtime = getRuntimeSignOutHooks();
  if (runtime) {
    const choice = args.skipRuntimeConfirm ? { deleteChatHistory: false } : await runtime.confirmRuntimeSignOut();
    if (choice === null) return null;
    await runtime.disposeRuntimeForSignOut(choice);
  }
  client.clearCsrfCache();
  const cloud = isHostedBackend(queryClient);
  try {
    if (cloud) await postCloudSignOut(client);
    else await postSignOut(client);
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
  // CLOUD-014: this tab signed out on purpose, so a later /login visit
  // shows the card instead of resuming silently. Written after the desktop
  // webview wipe, which clears sessionStorage; tab-scoped and best-effort,
  // so other tabs keep their own state.
  markExplicitLogout(tabSessionStorage());
  if (cloud) {
    globalThis.window.location.replace(CLOUD_SIGN_OUT_LANDING);
    return CLOUD_SIGN_OUT_LANDING;
  }
  return signOutLandingTarget(platform.kind);
}

/**
 * Sign out everywhere (AUTH-017): end the server session, then clear every
 * client store and the query cache, then the desktop local teardown.
 * Resolves to the signed-out landing target, or null when the user
 * cancels the desktop runtime confirmation; a failed POST toasts and
 * rejects instead, so the caller leaves the user where they are.
 */
export function useSignOut() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => performSignOut({ client: getAppClient(), queryClient, teardown: platform.sessionTeardown }),
  });
}
