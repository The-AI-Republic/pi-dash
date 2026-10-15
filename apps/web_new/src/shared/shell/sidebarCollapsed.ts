// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar collapse flag (shell spec, SHELL-089). The palette's toggle entry
// and its chord flip this; the shell hides the sidebar while it is set.
// Minimal on purpose: resize, peek and local persistence belong to the
// sidebar slices (NEWFRONT-287/290, SHELL-062/068/098), which extend this
// store instead of forking it.

import { create } from "zustand";

import { registerStoreReset } from "../../core/session/store.js";

interface SidebarCollapsedState {
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  toggleCollapsed: () => void;
}

const initial = { collapsed: false };

export const useSidebarCollapsed = create<SidebarCollapsedState>()((set) => ({
  ...initial,
  setCollapsed: (collapsed: boolean) => set({ collapsed }),
  toggleCollapsed: () => set((state) => ({ collapsed: !state.collapsed })),
}));

registerStoreReset(() => {
  useSidebarCollapsed.setState(initial);
});
