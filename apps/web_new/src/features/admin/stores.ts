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

import { readStoredSidebarCollapsed, writeStoredSidebarCollapsed } from "./adminShell.js";
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

// --- Sidebar collapsed state (ADM-063) ---------------------------------------

interface AdminSidebarState {
  /** Narrowed to the icon rail; persisted across reloads. */
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  toggleCollapsed: () => void;
}

export const useAdminSidebarStore = create<AdminSidebarState>()((set) => ({
  collapsed: readStoredSidebarCollapsed(),
  setCollapsed: (collapsed) => {
    writeStoredSidebarCollapsed(collapsed);
    set({ collapsed });
  },
  toggleCollapsed: () => {
    const { collapsed, setCollapsed } = useAdminSidebarStore.getState();
    setCollapsed(!collapsed);
  },
}));

// Sign-out restores the persisted flag instead of clearing it: the old
// console rebuilds the store and re-reads the preference at bootstrap,
// so the rail keeps its width and only the theme resets to system.
registerAdminStoreReset(() => {
  useAdminSidebarStore.setState({ collapsed: readStoredSidebarCollapsed() });
});

// --- Create-workspace draft (ADM-043/045) ------------------------------------

export interface WorkspaceCreateDraft {
  name: string;
  slug: string;
  organizationSize: string;
}

/** Blank draft: every field empty, no size band picked. */
export const WORKSPACE_CREATE_BLANK_DRAFT: WorkspaceCreateDraft = {
  name: "",
  slug: "",
  organizationSize: "",
};

interface WorkspaceCreateDraftState extends WorkspaceCreateDraft {
  /**
   * Merge typed values into the draft. The store outlives the form, so
   * navigating away and back keeps what was typed.
   */
  setDraft: (patch: Partial<WorkspaceCreateDraft>) => void;
  /** Clear back to blank; the form calls this after a successful create. */
  resetDraft: () => void;
}

export const useWorkspaceCreateDraftStore = create<WorkspaceCreateDraftState>()((set) => ({
  ...WORKSPACE_CREATE_BLANK_DRAFT,
  setDraft: (patch) => set(patch),
  resetDraft: () => set({ ...WORKSPACE_CREATE_BLANK_DRAFT }),
}));

registerAdminStoreReset(() => {
  useWorkspaceCreateDraftStore.getState().resetDraft();
});
