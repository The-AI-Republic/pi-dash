// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
