// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App rail preferences (SHELL-063, from parity/specs/shell.md §5): the
// strip density (captions start on) and the per-workspace undock flag.
// No network calls; both persist per device and skip the sign-out reset
// list like the other chrome preferences.
import { create } from "zustand";

import { loadPreference, storePreference } from "./preferenceStorage.js";

/** Rail strip density: captions under the icons, or icons alone. */
export type RailDensity = "labeled" | "icons";

interface RailState {
  density: RailDensity;
  undockedByWorkspace: Record<string, boolean>;
  setDensity: (density: RailDensity) => void;
  setUndocked: (workspaceSlug: string, undocked: boolean) => void;
}

interface RailSnapshot {
  density: RailDensity;
  undockedByWorkspace: Record<string, boolean>;
}

const STORAGE_KEY = "pidash:rail";

function isSnapshot(value: unknown): value is RailSnapshot {
  if (typeof value !== "object" || value === null) return false;
  const snapshot = value as Record<string, unknown>;
  if (snapshot.density !== "labeled" && snapshot.density !== "icons") return false;
  if (typeof snapshot.undockedByWorkspace !== "object" || snapshot.undockedByWorkspace === null) return false;
  return Object.values(snapshot.undockedByWorkspace).every((entry) => typeof entry === "boolean");
}

const initial = loadPreference(STORAGE_KEY, { density: "labeled", undockedByWorkspace: {} }, isSnapshot);

export const useRailState = create<RailState>()((set) => ({
  ...initial,
  setDensity: (density: RailDensity) => set({ density }),
  setUndocked: (workspaceSlug: string, undocked: boolean) =>
    set((state) => ({ undockedByWorkspace: { ...state.undockedByWorkspace, [workspaceSlug]: undocked } })),
}));

useRailState.subscribe((state) => {
  storePreference(STORAGE_KEY, { density: state.density, undockedByWorkspace: state.undockedByWorkspace });
});

/** True once the workspace undocked its rail; docked everywhere by default. */
export function isRailUndocked(state: Pick<RailState, "undockedByWorkspace">, workspaceSlug: string): boolean {
  return state.undockedByWorkspace[workspaceSlug] ?? false;
}
