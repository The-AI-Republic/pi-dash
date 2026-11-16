// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Address reader for the pages list (parity/specs/pages.md §2.6). Kept
// separate from the list model so the eager route's validateSearch never
// imports the screen model and row helpers (size gate).

export type PageListTab = "public" | "private" | "archived";

/** Tab labels are pinned by the oracle and the spec. */
export const PAGE_LIST_TABS: ReadonlyArray<{ key: PageListTab; label: string }> = [
  { key: "public", label: "Public" },
  { key: "private", label: "Private" },
  { key: "archived", label: "Archived" },
];

/** The only query key the list owns; search, sort and filters stay local. */
export interface PageListSearch {
  type: PageListTab;
}

/**
 * Read the tab from an arbitrary search record. Missing, repeated and
 * unknown values all land on public; the route then writes only the
 * canonical value back when the user selects a different tab.
 */
export function parsePageListSearch(input: Record<string, unknown>): PageListSearch {
  const raw = input["type"];
  const value = Array.isArray(raw) ? raw[0] : raw;
  if (value === "private") return { type: "private" };
  if (value === "archived") return { type: "archived" };
  return { type: "public" };
}
