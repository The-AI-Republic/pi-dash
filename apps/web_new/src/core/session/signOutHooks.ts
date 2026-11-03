// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runtime sign-out hooks (RUN-047). The runners feature owns the desktop
// runtime disposal — the confirm dialog and the pre-POST teardown — but the
// sign-out sequence lives in the auth feature, which must not depend on a
// domain feature. So the runners feature registers its hooks here (like the
// store resets in store.ts) and the sequence calls them when present. The
// web build never registers, so sign-out reads the same in both builds.

/** What the user chose when confirming a desktop sign-out. */
export interface RuntimeSignOutChoice {
  deleteChatHistory: boolean;
}

/** Desktop runtime steps the sign-out sequence runs when registered. */
export interface RuntimeSignOutHooks {
  /**
   * Confirm a user-started sign-out (and whether to delete that account's
   * on-machine chat history). Resolves null when the user cancels.
   */
  confirmRuntimeSignOut(): Promise<RuntimeSignOutChoice | null>;
  /**
   * Tear the local runtime down before the sign-out POST (server enrollment
   * revocation needs a live session). Best-effort: never rejects, so the
   * user is never trapped.
   */
  disposeRuntimeForSignOut(choice: RuntimeSignOutChoice): Promise<void>;
}

let hooks: RuntimeSignOutHooks | null = null;

/**
 * Register the runtime hooks (once, at the owning feature's module scope).
 * Returns an unregister function for tests.
 */
export function registerRuntimeSignOutHooks(next: RuntimeSignOutHooks | null): () => void {
  hooks = next;
  return () => {
    if (hooks === next) {
      hooks = null;
    }
  };
}

/** The registered hooks, or null when no runtime is present (web). */
export function getRuntimeSignOutHooks(): RuntimeSignOutHooks | null {
  return hooks;
}
