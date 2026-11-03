// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval-mode carry-over (RUN-047). The chat page keeps each runner's
// approval mode in local storage, and sign-out wipes the webview profile
// to drop the session cookies. Left at that, a user who picked "ask" signs
// back in under the full-access default and the engine stops asking. So the
// modes are handed to the host just before the wipe — it keeps them per
// account, outside the webview — and written back once that same account
// signs in again. Local storage stays the copy the chat page reads; the
// host's copy only survives the wipe.

import type { AgentRuntimeApi } from "../../../core/platform/index.js";

/**
 * Local-storage prefix for per-runner approval modes. The chat page (and
 * the parity probes) read these keys, so the prefix is protocol: never
 * rename it.
 */
export const APPROVAL_MODE_STORAGE_PREFIX = "pidash:chat-approval-mode:";

/** Protocol vocabulary for the approval modes (kept exactly). */
const APPROVAL_MODES = new Set(["ask", "workspace", "full_access"]);

export interface ApprovalModeCarryInput {
  account: string;
  runtime: AgentRuntimeApi;
  /** Local-storage override (tests only); the page store otherwise. */
  storage?: Storage | undefined;
}

/** Hand the modes in local storage to the host. Call before the webview is wiped. */
export async function stashApprovalModes(input: ApprovalModeCarryInput): Promise<void> {
  const { account, runtime } = input;
  const store = input.storage ?? globalThis.localStorage;
  if (!account || !store) return;
  const modes: Record<string, string> = {};
  for (let index = 0; index < store.length; index++) {
    const key = store.key(index);
    if (!key || !key.startsWith(APPROVAL_MODE_STORAGE_PREFIX)) continue;
    const mode = store.getItem(key);
    if (mode !== null && APPROVAL_MODES.has(mode)) {
      modes[key.slice(APPROVAL_MODE_STORAGE_PREFIX.length)] = mode;
    }
  }
  if (Object.keys(modes).length > 0) {
    await runtime.saveApprovalModes(account, modes);
  }
}

/**
 * Write the host's copy back for the account that just signed in. A mode
 * already in local storage is newer than the host's and is left alone.
 */
export async function restoreApprovalModes(input: ApprovalModeCarryInput): Promise<void> {
  const { account, runtime } = input;
  const store = input.storage ?? globalThis.localStorage;
  if (!account || !store) return;
  const modes = await runtime.loadApprovalModes(account);
  for (const [runnerId, mode] of Object.entries(modes)) {
    const key = APPROVAL_MODE_STORAGE_PREFIX + runnerId;
    if (APPROVAL_MODES.has(mode) && store.getItem(key) === null) {
      store.setItem(key, mode);
    }
  }
}
