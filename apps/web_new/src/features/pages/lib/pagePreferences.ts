// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page display preferences (parity/specs/pages.md §9.1, PAGE-035/072). The
// page area keeps four display preferences per user in local storage;
// this slice owns the one it renders — the sticky formatting bar — and
// leaves the rest of the stored object untouched when it is read, so the
// preferences slice (NEWFRONT-848) can add their toggles without a
// migration. The key and the shape are the old app's (a stored page
// config the user already has keeps working).

import type { KeyValueStore } from "../../../core/platform/types.js";

/** Local storage key the page preferences live under (spec §9.1). */
export const PAGE_PREFERENCES_KEY = "pages_config";

/** The whole stored object; absent fields keep their defaults. */
export interface PagePreferences {
  full_width: boolean;
  font_size: string;
  font_style: string;
  sticky_toolbar: boolean;
}

/** Defaults a user without a stored preference sees. */
export const PAGE_PREFERENCES_DEFAULTS: PagePreferences = {
  full_width: false,
  font_size: "large-font",
  font_style: "sans-serif",
  sticky_toolbar: true,
};

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

/** The sticky-toolbar preference, defaulting to on (PAGE-035). */
export function stickyToolbarOf(stored: string | null): boolean {
  const config = parsePagePreferences(stored);
  return typeof config.sticky_toolbar === "boolean" ? config.sticky_toolbar : PAGE_PREFERENCES_DEFAULTS.sticky_toolbar;
}

/**
 * Whether the sticky formatting bar renders for this viewer: an editable
 * page plus the preference on (PAGE-035). The floating pane toggle is
 * independent of this — it is what remains when the bar is gone.
 */
export function formattingBarVisible(input: { editable: boolean; stickyToolbar: boolean }): boolean {
  return input.editable && input.stickyToolbar;
}

/** Read the sticky preference from the platform store. */
export async function readStickyToolbar(store: KeyValueStore): Promise<boolean> {
  return stickyToolbarOf(await store.get(PAGE_PREFERENCES_KEY));
}

/**
 * Read the sticky preference on mount. The preference lives in local
 * storage under its own un-namespaced key (the old app wrote it there and
 * a user's stored choice must keep working, spec §9.1); an unavailable
 * storage keeps the default rather than failing the page.
 */
export function readStickyToolbarFromWindow(): boolean {
  if (typeof globalThis.window === "undefined") return PAGE_PREFERENCES_DEFAULTS.sticky_toolbar;
  try {
    return stickyToolbarOf(globalThis.window.localStorage.getItem(PAGE_PREFERENCES_KEY));
  } catch {
    return PAGE_PREFERENCES_DEFAULTS.sticky_toolbar;
  }
}
