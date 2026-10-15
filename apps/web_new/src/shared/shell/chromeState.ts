// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shell chrome state (SHELL-066, from parity/specs/shell.md §5): the
// sidebar collapse flag the top-bar toggle flips, and the peek-open flag
// the same click always clears. Both persist per device and deliberately
// skip the sign-out reset list: they are device preferences, not session
// state, and the old app likewise keeps its collapse and peek flags in
// local storage across sign-out.
import { create } from "zustand";

import { loadPreference, storePreference } from "./preferenceStorage.js";

interface ShellChromeState {
  sidebarCollapsed: boolean;
  peekOpen: boolean;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebarCollapsed: () => void;
  setPeekOpen: (open: boolean) => void;
  clearPeek: () => void;
}

interface ShellChromeSnapshot {
  sidebarCollapsed: boolean;
  peekOpen: boolean;
}

const STORAGE_KEY = "pidash:shell-chrome";

function isSnapshot(value: unknown): value is ShellChromeSnapshot {
  if (typeof value !== "object" || value === null) return false;
  const snapshot = value as Record<string, unknown>;
  return typeof snapshot.sidebarCollapsed === "boolean" && typeof snapshot.peekOpen === "boolean";
}

const initial = loadPreference(STORAGE_KEY, { sidebarCollapsed: false, peekOpen: false }, isSnapshot);

export const useShellChrome = create<ShellChromeState>()((set) => ({
  ...initial,
  setSidebarCollapsed: (collapsed: boolean) => set({ sidebarCollapsed: collapsed }),
  toggleSidebarCollapsed: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
  setPeekOpen: (open: boolean) => set({ peekOpen: open }),
  clearPeek: () => set({ peekOpen: false }),
}));

useShellChrome.subscribe((state) => {
  storePreference(STORAGE_KEY, { sidebarCollapsed: state.sidebarCollapsed, peekOpen: state.peekOpen });
});
