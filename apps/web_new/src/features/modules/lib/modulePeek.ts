// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// URL-driven module peek (parity/specs/modules.md §5, MOD-019). The peek is
// a search key on the list screen, never a path segment: opening writes
// `peekModule`, closing deletes only that key, and every other key (the
// layout, the search, the ordering) survives untouched. Writers replace the
// history entry rather than pushing one, so Back leaves the screen instead
// of peeling the peek off.
//
// The same rules apply on the archived screen, where the panel reads the
// archived record; the path is what selects the read, not the key.

import { MODULE_PEEK_PARAM, readPeekModule } from "./moduleEntry.js";

/**
 * Search with one module peeked, or with the key removed. The id is checked
 * like any other `peekModule` value; an unusable id opens nothing.
 */
export function withModulePeek(search: Record<string, unknown>, moduleId: string | null): Record<string, unknown> {
  const next = { ...search };
  if (moduleId === null || readPeekModule({ [MODULE_PEEK_PARAM]: moduleId }) === null) {
    delete next[MODULE_PEEK_PARAM];
    return next;
  }
  next[MODULE_PEEK_PARAM] = moduleId;
  return next;
}

/** The module a screen's search peeks, or null when none is open. */
export function modulePeekFromSearch(search: Record<string, unknown>): string | null {
  return readPeekModule(search);
}

/**
 * Board grid class list. Cards sit in one column on a phone, two from the
 * medium band and three on a wide screen; an open peek takes the wide band
 * down to two so a card behind the panel is not squeezed into a third of
 * the remaining width. Closing the peek restores the three-column band.
 */
export function moduleBoardGridClass(args: { peekOpen: boolean }): string {
  const wideColumns = args.peekOpen ? "xl:grid-cols-2" : "xl:grid-cols-3";
  return `grid grid-cols-1 gap-(--space-4) md:grid-cols-2 ${wideColumns}`;
}
