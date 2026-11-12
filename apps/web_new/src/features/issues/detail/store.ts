// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail UI state (parity/specs/issues.md §13, ISS-144). The properties
// sidebar collapse flag is global: the detail root, the peek panel,
// and the responsive auto-collapse all read and write this one flag.
// Sign-out resets it through the session registry.

import { create } from "zustand";

import { registerStoreReset } from "../../../core/session/index.js";

interface DetailUiState {
  sidebarCollapsed: boolean;
  setSidebarCollapsed: (collapsed: boolean) => void;
}

export const useDetailUiStore = create<DetailUiState>()((set) => ({
  sidebarCollapsed: false,
  setSidebarCollapsed: (collapsed: boolean) => set({ sidebarCollapsed: collapsed }),
}));

registerStoreReset(() => {
  useDetailUiStore.setState({ sidebarCollapsed: false });
});
