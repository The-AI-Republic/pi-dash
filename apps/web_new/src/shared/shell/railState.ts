// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App rail preferences (SHELL-063, from parity/specs/shell.md §5): the
// strip density (captions start on) and the per-workspace undock flag.
// No network calls; both persist per device and skip the sign-out reset
// list like the other chrome preferences.
import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import { preferenceStorage } from "./preferenceStorage.js";

/** Rail strip density: captions under the icons, or icons alone. */
export type RailDensity = "labeled" | "icons";

interface RailState {
  density: RailDensity;
  undockedByWorkspace: Record<string, boolean>;
  setDensity: (density: RailDensity) => void;
  setUndocked: (workspaceSlug: string, undocked: boolean) => void;
}

export const useRailState = create<RailState>()(
  persist(
    (set) => ({
      density: "labeled",
      undockedByWorkspace: {},
      setDensity: (density: RailDensity) => set({ density }),
      setUndocked: (workspaceSlug: string, undocked: boolean) =>
        set((state) => ({ undockedByWorkspace: { ...state.undockedByWorkspace, [workspaceSlug]: undocked } })),
    }),
    {
      name: "pidash:rail",
      storage: createJSONStorage(() => preferenceStorage()),
      partialize: (state) => ({ density: state.density, undockedByWorkspace: state.undockedByWorkspace }),
    }
  )
);

/** True once the workspace undocked its rail; docked everywhere by default. */
export function isRailUndocked(state: Pick<RailState, "undockedByWorkspace">, workspaceSlug: string): boolean {
  return state.undockedByWorkspace[workspaceSlug] ?? false;
}
