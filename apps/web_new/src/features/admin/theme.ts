// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin theme preference (ADM-010 reset, ADM-065 toggle, from
// parity/specs/admin.md §1.10): the console's own light/dark/system
// mode, persisted under the raw storage key the parity scenario reads
// and applied to the document on every change. The member theme store
// is not reused. The dashboard layout applies the stored mode on mount
// so the choice survives a reload; sign-out resets it to system.
import { applyTheme, resolveTheme, type ThemeMode } from "@pidash/kit";
import { create } from "zustand";

import { registerAdminStoreReset } from "./stores.js";

/** Raw storage key for the persisted mode. Pinned by the parity driver. */
export const ADMIN_THEME_KEY = "theme";

export type AdminThemeMode = ThemeMode;

function domStore(): Pick<Storage, "getItem" | "setItem"> | null {
  return globalThis.window?.localStorage ?? null;
}

/** Read the persisted mode; absent or unknown values fall back to system. */
export function readStoredAdminTheme(): AdminThemeMode {
  const raw = domStore()?.getItem(ADMIN_THEME_KEY);
  return raw === "light" || raw === "dark" || raw === "system" ? raw : "system";
}

function writeStoredAdminTheme(mode: AdminThemeMode): void {
  domStore()?.setItem(ADMIN_THEME_KEY, mode);
}

/**
 * Apply a mode to the document: resolved to a concrete scheme first
 * (system defers to the OS), the same shape as the app boot mark.
 * No document, no-op.
 */
export function applyAdminThemePreference(mode: AdminThemeMode): void {
  const root = globalThis.document?.documentElement;
  if (root === undefined) {
    return;
  }
  const scheme = resolveTheme(mode);
  root.style.setProperty("color-scheme", scheme);
  applyTheme(scheme, root);
}

/**
 * Apply the persisted mode to the document. The dashboard layout calls
 * this on mount so a reloaded console keeps its theme.
 */
export function applyStoredAdminTheme(): void {
  applyAdminThemePreference(readStoredAdminTheme());
}

interface AdminThemeState {
  mode: AdminThemeMode;
  setMode: (mode: AdminThemeMode) => void;
}

export const useAdminThemeStore = create<AdminThemeState>()((set) => ({
  mode: readStoredAdminTheme(),
  setMode: (mode) => {
    writeStoredAdminTheme(mode);
    applyAdminThemePreference(mode);
    set({ mode });
  },
}));

registerAdminStoreReset(() => {
  useAdminThemeStore.getState().setMode("system");
});
