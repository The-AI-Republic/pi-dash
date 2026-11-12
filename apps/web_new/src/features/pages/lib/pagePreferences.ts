// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page display preferences (parity/specs/pages.md §9.1, PAGE-072,
// PAGE-035). Four display choices live per user in the browser profile
// under the historical `pages_config` key: type size, typeface, canvas
// width and the sticky formatting bar. They never travel to the server
// and never touch saved content — reading and writing is local, and the
// editor reflows from the applied values. Only width and the bar have
// visible toggles (the detail options menu); size and face are read from
// the stored configuration and have no switcher in this area.

import * as React from "react";

import type { KeyValueStore } from "../../../core/platform/types.js";
import { storePreference } from "../../../shared/shell/preferenceStorage.js";

/** Local storage key the page preferences live under (spec §9.1). */
export const PAGE_PREFERENCES_KEY = "pages_config";

/** The two stored type sizes (the old app's value names). */
export type PageFontSize = "small-font" | "large-font";

/** The two stored typefaces (the old app's value names). */
export type PageFontStyle = "sans-serif" | "serif";

/** The whole stored object; absent fields keep their defaults. */
export interface PagePreferences {
  full_width: boolean;
  font_size: PageFontSize;
  font_style: PageFontStyle;
  sticky_toolbar: boolean;
}

/** Defaults a user without a stored preference sees. */
export const PAGE_PREFERENCES_DEFAULTS: PagePreferences = {
  full_width: false,
  font_size: "large-font",
  font_style: "sans-serif",
  sticky_toolbar: true,
};

/** The canvas measures the width preference chooses between (PAGE-072). */
export const PAGE_CANVAS_WIDTH_PX = { readable: "720px", full: "1152px" } as const;

/**
 * Read the raw stored object. A missing, unparsable or non-object value
 * reads as empty, so a corrupt entry never breaks the page.
 */
export function parsePagePreferences(stored: string | null): Partial<PagePreferences> {
  if (stored === null || stored.trim() === "") return {};
  try {
    const parsed: unknown = JSON.parse(stored);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    return parsed as Partial<PagePreferences>;
  } catch {
    return {};
  }
}

function fontSizes(value: unknown): PageFontSize {
  return value === "small-font" || value === "large-font" ? value : PAGE_PREFERENCES_DEFAULTS.font_size;
}

function fontStyles(value: unknown): PageFontStyle {
  return value === "sans-serif" || value === "serif" ? value : PAGE_PREFERENCES_DEFAULTS.font_style;
}

/**
 * The complete preference set a record describes: every stored field that
 * holds a usable value, every absent or malformed field at its default.
 * A partial stored object (an older build wrote only some keys) reads as
 * the rest of the defaults rather than as a missing configuration.
 */
export function pagePreferencesOf(stored: string | null): PagePreferences {
  const config = parsePagePreferences(stored);
  return {
    full_width: typeof config.full_width === "boolean" ? config.full_width : PAGE_PREFERENCES_DEFAULTS.full_width,
    font_size: fontSizes(config.font_size),
    font_style: fontStyles(config.font_style),
    sticky_toolbar:
      typeof config.sticky_toolbar === "boolean" ? config.sticky_toolbar : PAGE_PREFERENCES_DEFAULTS.sticky_toolbar,
  };
}

/** The sticky-toolbar preference, defaulting to on (PAGE-035). */
export function stickyToolbarOf(stored: string | null): boolean {
  return pagePreferencesOf(stored).sticky_toolbar;
}

/**
 * Whether the sticky formatting bar renders for this viewer: an editable
 * page plus the preference on (PAGE-035). The floating pane toggle is
 * independent of this — it is what remains when the bar is gone.
 */
export function formattingBarVisible(input: { editable: boolean; stickyToolbar: boolean }): boolean {
  return input.editable && input.stickyToolbar;
}

/**
 * The canvas measure for the current preferences and pane state: the wide
 * measure whenever the author asked for full width or the pane is open,
 * the readable measure otherwise (PAGE-045/072).
 */
export function pageCanvasWidth(input: { fullWidth: boolean; paneOpen: boolean }): string {
  return input.fullWidth || input.paneOpen ? PAGE_CANVAS_WIDTH_PX.full : PAGE_CANVAS_WIDTH_PX.readable;
}

/** Read the sticky preference from the platform store. */
export async function readStickyToolbar(store: KeyValueStore): Promise<boolean> {
  return stickyToolbarOf(await store.get(PAGE_PREFERENCES_KEY));
}

function storedInWindow(): string | null {
  if (typeof globalThis.window === "undefined") return null;
  try {
    return globalThis.window.localStorage.getItem(PAGE_PREFERENCES_KEY);
  } catch {
    return null;
  }
}

/**
 * Read the whole preference set on mount. The preference lives in local
 * storage under its own un-namespaced key (the old app wrote it there and
 * a user's stored choice must keep working, spec §9.1); an unavailable
 * storage keeps the defaults rather than failing the page.
 */
export function readPagePreferencesFromWindow(): PagePreferences {
  return pagePreferencesOf(storedInWindow());
}

/** The sticky preference alone, defaulting to on (kept for PAGE-035 callers). */
export function readStickyToolbarFromWindow(): boolean {
  return readPagePreferencesFromWindow().sticky_toolbar;
}

/**
 * Merge one change into the stored set and return the full new set. The
 * write is local only and keeps the other fields a user already chose, so
 * the two visible toggles never reset the size or face.
 */
export function writePagePreferences(patch: Partial<PagePreferences>): PagePreferences {
  const next = { ...readPagePreferencesFromWindow(), ...patch };
  storePreference(PAGE_PREFERENCES_KEY, next);
  return next;
}

/**
 * The local preference set as React state for one page. A page switch
 * re-reads the store (another tab may have changed it, and the shell
 * remounts on navigation); changing a preference reflows immediately
 * because the applied values come back from this state. The setter writes
 * the merged object, never the whole defaults.
 */
export function usePagePreferences(pageId: string): {
  preferences: PagePreferences;
  setPreference: (patch: Partial<PagePreferences>) => void;
} {
  const [preferences, setPreferences] = React.useState<PagePreferences>(readPagePreferencesFromWindow);
  React.useEffect(() => {
    setPreferences(readPagePreferencesFromWindow());
  }, [pageId]);
  const setPreference = React.useCallback((patch: Partial<PagePreferences>) => {
    setPreferences(writePagePreferences(patch));
  }, []);
  return { preferences, setPreference };
}
