/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Carries the local-chat approval mode across a desktop sign-out.
 *
 * The chat page keeps the chosen mode in `localStorage`, and sign-out wipes the
 * webview's whole profile to get rid of the session cookies. Left at that, a
 * user who picked "Ask" signs back in under the full-access default and the
 * engine stops asking. So the modes are handed to the host just before the
 * wipe — it keeps them per account, outside the webview — and written back
 * once that same account is signed in again.
 *
 * `localStorage` stays the copy the chat page reads; the host's is only what
 * survives the wipe.
 */

import type { TApprovalMode } from "@pi-dash/types";

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

/** Must match `approvalModeStorageKey` in `runners/chat/[runnerId]/page.tsx`. */
const STORAGE_PREFIX = "pidash:chat-approval-mode:";

function invoke<T>(command: string, args?: Record<string, unknown>) {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

function isApprovalMode(value: unknown): value is TApprovalMode {
  return value === "ask" || value === "workspace" || value === "full_access";
}

/** Hand the modes in `localStorage` to the host. Call before the webview is wiped. */
export async function stashApprovalModes(account: string): Promise<void> {
  if (!account) return;
  const modes: Record<string, TApprovalMode> = {};
  for (let index = 0; index < window.localStorage.length; index++) {
    const key = window.localStorage.key(index);
    if (!key?.startsWith(STORAGE_PREFIX)) continue;
    const mode = window.localStorage.getItem(key);
    if (isApprovalMode(mode)) modes[key.slice(STORAGE_PREFIX.length)] = mode;
  }
  if (Object.keys(modes).length) await invoke<void>("chat_approval_modes_save", { account, modes });
}

/**
 * Write the host's copy back for the account that just signed in. A mode
 * already in `localStorage` is newer than the host's and is left alone.
 */
export async function restoreApprovalModes(account: string): Promise<void> {
  if (!account) return;
  const modes = await invoke<Record<string, unknown>>("chat_approval_modes_load", { account });
  for (const [runnerId, mode] of Object.entries(modes ?? {})) {
    const key = STORAGE_PREFIX + runnerId;
    if (isApprovalMode(mode) && window.localStorage.getItem(key) === null) window.localStorage.setItem(key, mode);
  }
}
