// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shell chrome state (SHELL-066/068/098, from parity/specs/shell.md §5-6):
// per-panel collapse, peek and drag width. Each sidebar panel (main today;
// extended, project, profile, analytics and detail panels adopt the same
// store when they land) persists its own snapshot under its own key, so a
// reload restores every panel independently. Device preferences, not
// session state: deliberately absent from the sign-out reset list, like
// the other chrome preferences. The key scheme replaces the single
// pidash:shell-chrome record; web_new is unshipped, so no migration.
import { create } from "zustand";

import { loadPreference, storePreference } from "./preferenceStorage.js";

/** Drag bounds for the sidebar width (SHELL-068). */
export const SIDEBAR_MIN_WIDTH = 236;
export const SIDEBAR_MAX_WIDTH = 350;
/** Resting width before the first drag. */
export const SIDEBAR_DEFAULT_WIDTH = 256;

/** The main workspace sidebar panel. */
export const MAIN_SIDEBAR_PANEL = "main";

/** One panel's persisted chrome: collapse, peek and drag width. */
export interface SidebarPanelSnapshot {
  sidebarCollapsed: boolean;
  peekOpen: boolean;
  sidebarWidth: number;
}

/** Storage key for one panel's snapshot. */
export function sidebarPanelStorageKey(panel: string): string {
  return `pidash:sidebar-panel:${panel}`;
}

/** Fold any width into the draggable range; foreign values rest centered. */
export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(Math.max(width, SIDEBAR_MIN_WIDTH), SIDEBAR_MAX_WIDTH);
}

const DEFAULT_SNAPSHOT: SidebarPanelSnapshot = {
  sidebarCollapsed: false,
  peekOpen: false,
  sidebarWidth: SIDEBAR_DEFAULT_WIDTH,
};

function isSnapshot(value: unknown): value is SidebarPanelSnapshot {
  if (typeof value !== "object" || value === null) return false;
  const snapshot = value as Record<string, unknown>;
  return (
    typeof snapshot.sidebarCollapsed === "boolean" &&
    typeof snapshot.peekOpen === "boolean" &&
    typeof snapshot.sidebarWidth === "number" &&
    Number.isFinite(snapshot.sidebarWidth)
  );
}

/** Read one panel's snapshot, defaulting (and clamping) anything foreign. */
export function loadSidebarPanel(panel: string): SidebarPanelSnapshot {
  const loaded = loadPreference(sidebarPanelStorageKey(panel), DEFAULT_SNAPSHOT, isSnapshot);
  return { ...loaded, sidebarWidth: clampSidebarWidth(loaded.sidebarWidth) };
}

export interface SidebarPanelState extends SidebarPanelSnapshot {
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebarCollapsed: () => void;
  setPeekOpen: (open: boolean) => void;
  clearPeek: () => void;
  setSidebarWidth: (width: number) => void;
}

/**
 * An independent persisted store for one sidebar panel. The toggle flips
 * collapse; peek opens only through the edge trigger and any toggle
 * clears it (SHELL-066); widths clamp into the draggable range.
 */
export function createSidebarPanelStore(panel: string) {
  const key = sidebarPanelStorageKey(panel);
  const store = create<SidebarPanelState>()((set) => ({
    ...loadSidebarPanel(panel),
    setSidebarCollapsed: (collapsed: boolean) => set({ sidebarCollapsed: collapsed }),
    toggleSidebarCollapsed: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed, peekOpen: false })),
    setPeekOpen: (open: boolean) => set({ peekOpen: open }),
    clearPeek: () => set({ peekOpen: false }),
    setSidebarWidth: (width: number) => set({ sidebarWidth: clampSidebarWidth(width) }),
  }));
  store.subscribe((state) => {
    storePreference(key, {
      sidebarCollapsed: state.sidebarCollapsed,
      peekOpen: state.peekOpen,
      sidebarWidth: state.sidebarWidth,
    });
  });
  return store;
}

/** The main workspace sidebar's chrome store. */
export const useShellChrome = createSidebarPanelStore(MAIN_SIDEBAR_PANEL);

type SidebarPanelStore = ReturnType<typeof createSidebarPanelStore>;

const panelStores = new Map<string, SidebarPanelStore>([[MAIN_SIDEBAR_PANEL, useShellChrome]]);

/**
 * The persisted store for one panel, shared per id so every mount of a
 * panel reads and writes the same snapshot. The main panel's store is
 * useShellChrome itself.
 */
export function sidebarPanelStore(panel: string): SidebarPanelStore {
  const cached = panelStores.get(panel);
  if (cached !== undefined) return cached;
  const store = createSidebarPanelStore(panel);
  panelStores.set(panel, store);
  return store;
}
