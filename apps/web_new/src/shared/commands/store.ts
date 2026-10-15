// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command-palette UI state (Data layer > Client state). Only the open flag,
// the filter text, the current sub-page and the server-search scope live
// here; the commands and pages themselves live in the registry. Every close
// resets to a fresh state (shell spec, SHELL-082); sign-out resets the same.

import { create } from "zustand";

import { registerStoreReset } from "../../core/session/store.js";

interface CommandPaletteState {
  open: boolean;
  query: string;
  /** Active sub-page id, or null on the root command list. */
  page: string | null;
  /** True widens server search from the project to the workspace (SHELL-084). */
  workspaceSearch: boolean;
  /** The entity context is dismissed (chip + contextual group hidden). */
  contextDismissed: boolean;
  openPalette: () => void;
  closePalette: () => void;
  togglePalette: () => void;
  setQuery: (query: string) => void;
  /** Step into a sub-page; the query clears for the page's own filter. */
  openPage: (page: string) => void;
  /** Step back to the root list; the query clears (SHELL-083). */
  closePage: () => void;
  setWorkspaceSearch: (workspaceSearch: boolean) => void;
  /**
   * Hide the contextual group and chip until the next open, stepping
   * back to the root list so a contextual sub-page never orphans.
   */
  dismissContext: () => void;
}

const fresh = { query: "", page: null as string | null, workspaceSearch: false, contextDismissed: false };

const initial = { open: false, ...fresh };

export const useCommandPalette = create<CommandPaletteState>()((set) => ({
  ...initial,
  openPalette: () => set({ open: true, ...fresh }),
  closePalette: () => set({ open: false, ...fresh }),
  togglePalette: () => set((state) => ({ open: !state.open, ...fresh })),
  setQuery: (query: string) => set({ query }),
  openPage: (page: string) => set({ page, query: "" }),
  closePage: () => set({ page: null, query: "" }),
  setWorkspaceSearch: (workspaceSearch: boolean) => set({ workspaceSearch }),
  dismissContext: () => set({ contextDismissed: true, page: null, query: "" }),
}));

registerStoreReset(() => {
  useCommandPalette.setState(initial);
});
