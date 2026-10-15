// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin client-state reset (ADM-010, from parity/specs/admin.md §0.3 and
// §1.10). The admin registry mirrors the member reset-registry shape,
// but admin-scoped: admin sign-out clears exactly the admin query
// subtree plus the registered admin UI stores, leaving the member
// session and its data alone. Every admin UI store registers once at
// module scope.
import type { QueryClient } from "@tanstack/react-query";
import { create } from "zustand";

import { adminKeys } from "./queries.js";

/** An admin UI store reset, registered so sign-out clears it. */
export type AdminStoreReset = () => void;

const resetters = new Set<AdminStoreReset>();

/**
 * Join the admin sign-out reset list. Each admin UI store calls this
 * once at module scope. Returns an unregister function for tests.
 */
export function registerAdminStoreReset(reset: AdminStoreReset): () => void {
  resetters.add(reset);
  return () => {
    resetters.delete(reset);
  };
}

export function resetAdminStores(): void {
  for (const reset of resetters) {
    reset();
  }
}

/**
 * Sign-out reset: fresh admin UI stores, cleared admin identity and
 * bootstrap (the next mount re-reads them), member state untouched.
 */
export function resetAdminClientState(queryClient: QueryClient): void {
  resetAdminStores();
  queryClient.removeQueries({ queryKey: adminKeys.root });
}

// --- First-workspace popup flag (ADM-012) ------------------------------------

interface FirstWorkspaceState {
  /** Dismissed for the page lifetime; a reload starts un-dismissed. */
  dismissed: boolean;
  dismiss: () => void;
}

export const useFirstWorkspaceStore = create<FirstWorkspaceState>()((set) => ({
  dismissed: false,
  dismiss: () => set({ dismissed: true }),
}));

registerAdminStoreReset(() => {
  useFirstWorkspaceStore.setState({ dismissed: false });
});
