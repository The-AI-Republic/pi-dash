// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command-palette UI state (Data layer > Client state). Only the open flag
// and the filter text live here; the commands themselves live in the
// registry. Reset on sign-out like every feature store.

import { create } from "zustand";

import { registerStoreReset } from "../../core/session/store.js";

interface CommandPaletteState {
  open: boolean;
  query: string;
  openPalette: () => void;
  closePalette: () => void;
  togglePalette: () => void;
  setQuery: (query: string) => void;
}

const initial = { open: false, query: "" };

export const useCommandPalette = create<CommandPaletteState>()((set) => ({
  ...initial,
  openPalette: () => set({ open: true, query: "" }),
  closePalette: () => set({ open: false, query: "" }),
  togglePalette: () => set((state) => ({ open: !state.open, query: "" })),
  setQuery: (query: string) => set({ query }),
}));

registerStoreReset(() => {
  useCommandPalette.setState(initial);
});
