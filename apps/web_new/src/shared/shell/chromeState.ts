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
import { createJSONStorage, persist } from "zustand/middleware";

import { preferenceStorage } from "./preferenceStorage.js";

interface ShellChromeState {
  sidebarCollapsed: boolean;
  peekOpen: boolean;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebarCollapsed: () => void;
  setPeekOpen: (open: boolean) => void;
  clearPeek: () => void;
}

export const useShellChrome = create<ShellChromeState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      peekOpen: false,
      setSidebarCollapsed: (collapsed: boolean) => set({ sidebarCollapsed: collapsed }),
      toggleSidebarCollapsed: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
      setPeekOpen: (open: boolean) => set({ peekOpen: open }),
      clearPeek: () => set({ peekOpen: false }),
    }),
    {
      name: "pidash:shell-chrome",
      storage: createJSONStorage(() => preferenceStorage()),
      partialize: (state) => ({ sidebarCollapsed: state.sidebarCollapsed, peekOpen: state.peekOpen }),
    }
  )
);
