// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Favorites-section open flag (SHELL-032, SHELL-056 seam). Starring the
// first project opens the sidebar favorites section; the section itself
// (NEWFRONT-286) reads this flag to decide its initial open state. One
// module so both slices share the key and the stored shape: the string
// "1" means open, anything else (including missing) means closed.

/** localStorage key; shared by the card star and the sidebar section. */
export const FAVORITES_SECTION_OPEN_KEY = "web_new.favorites.open";

function storage(): Storage | null {
  return globalThis.localStorage ?? null;
}

/** True when a previous star asked the favorites section to open. */
export function isFavoritesSectionOpen(): boolean {
  try {
    return storage()?.getItem(FAVORITES_SECTION_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

/** Record whether the favorites section should render open. */
export function setFavoritesSectionOpen(open: boolean): void {
  try {
    const store = storage();
    if (store === null) return;
    if (open) store.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    else store.removeItem(FAVORITES_SECTION_OPEN_KEY);
  } catch {
    // Private modes deny storage; the section just stays closed.
  }
}
