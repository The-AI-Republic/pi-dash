// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-document paint cache (parity/specs/pages.md §2.7, §8.1, PAGE-079).
// The body skeleton is a cold-load surface only: once a page's document has
// been handed to the editor (or shown read-only) in this session, revisits
// paint straight from cache and never re-skeleton. The collaboration slice
// replaces the source of `isPageBodyReady` with its Yjs cache; the seam is
// the same module.

const readyPages = new Set<string>();

/** True once this session has painted the page's document. */
export function isPageBodyReady(pageId: string): boolean {
  return readyPages.has(pageId);
}

/** Mark the page's document as painted for the rest of the session. */
export function markPageBodyReady(pageId: string): void {
  readyPages.add(pageId);
}

/** Test/bootstrap reset; mutable state must not leak between mounts. */
export function resetPageBodyCache(): void {
  readyPages.clear();
}
