// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shortcut-dialog open flag (shell spec, SHELL-094). Split from the
// dialog itself so the shell reads this statically while the dialog
// content lazy-mounts on first open (Quality gates budget).

import { create } from "zustand";

import { registerStoreReset } from "../../core/session/store.js";

interface ShortcutsDialogState {
  open: boolean;
  openShortcuts: () => void;
  closeShortcuts: () => void;
}

const initial = { open: false };

export const useShortcutsDialog = create<ShortcutsDialogState>()((set) => ({
  ...initial,
  openShortcuts: () => set({ open: true }),
  closeShortcuts: () => set({ open: false }),
}));

registerStoreReset(() => {
  useShortcutsDialog.setState(initial);
});
