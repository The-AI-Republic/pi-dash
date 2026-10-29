// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Visitor theme for public boards (parity/specs/space.md SPC-011). A
// visitor has no profile, so the choice lives in a space-scoped storage
// key, never on the server: no stored choice follows the OS scheme live,
// the first toggle flips whatever is showing and stores it explicitly,
// and clearing the key returns the next load to the OS default. The
// document pin always names the resolved theme, so the toggle state and
// the rendered board agree at all times.
import { applyTheme, resolveTheme, watchSystemTheme } from "@pidash/kit";
import * as React from "react";

export type SpaceTheme = "light" | "dark";

/** Space-scoped storage key; the workspace theme is profile-bound. */
export const SPACE_THEME_STORAGE_KEY = "space-theme";

function storage(): Storage | null {
  return globalThis.localStorage ?? null;
}

/** The stored explicit choice, or null when the OS decides. */
export function readStoredSpaceTheme(): SpaceTheme | null {
  try {
    const raw = storage()?.getItem(SPACE_THEME_STORAGE_KEY);
    return raw === "light" || raw === "dark" ? raw : null;
  } catch {
    return null;
  }
}

/** Persist the explicit choice; private modes silently keep the default. */
export function writeStoredSpaceTheme(theme: SpaceTheme): void {
  try {
    storage()?.setItem(SPACE_THEME_STORAGE_KEY, theme);
  } catch {
    // Dismissed: the board still renders the resolved theme this visit.
  }
}

/** Resolved board theme: the explicit choice, else the live OS scheme. */
export function resolveSpaceTheme(stored: SpaceTheme | null): SpaceTheme {
  if (stored !== null) return stored;
  return resolveTheme("system");
}

/** True when the runtime can watch the OS scheme for changes. */
export function canWatchSystemTheme(): boolean {
  return typeof globalThis.window !== "undefined" && typeof globalThis.window.matchMedia === "function";
}

/**
 * Visitor theme state. Applies the resolved theme before paint, follows
 * OS changes live while nothing is stored, and flips the current theme
 * on every toggle — including the first click from the OS default.
 */
export function useSpaceTheme(): { theme: SpaceTheme; toggle: () => void } {
  const [stored, setStored] = React.useState<SpaceTheme | null>(readStoredSpaceTheme);
  const [system, setSystem] = React.useState<SpaceTheme>(() => resolveTheme("system"));
  React.useEffect(() => {
    if (stored !== null || !canWatchSystemTheme()) return;
    return watchSystemTheme(setSystem);
  }, [stored]);
  const theme = stored ?? system;
  React.useLayoutEffect(() => {
    applyTheme(theme);
  }, [theme]);
  // The app root and the workspace theme provider resolve a different
  // (profile-bound) key in passive effects, which run after this layout
  // effect and would overwrite the visitor's stored choice on every fresh
  // mount. Hold the pin while the switch is mounted: an external write of
  // any other value is corrected back to the resolved theme.
  React.useEffect(() => {
    if (typeof MutationObserver === "undefined") return;
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      if (root.getAttribute("data-theme") !== theme) applyTheme(theme);
    });
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, [theme]);
  const toggle = React.useCallback(() => {
    const next: SpaceTheme = theme === "light" ? "dark" : "light";
    writeStoredSpaceTheme(next);
    setStored(next);
  }, [theme]);
  return { theme, toggle };
}
