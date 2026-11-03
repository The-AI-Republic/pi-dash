// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body draft seam (parity/specs/pages.md §8.1). The editor reports
// its HTML here on every edit; the sync/save slice (PAGE-064,
// NEWFRONT-846) reads the draft, owns the socket-first saves and the
// REST fallback. This module deliberately owns no write: it keeps the
// newest local body per page for the rest of the session, so a remount
// reopens what was typed rather than the stored snapshot, and it lets
// the later slice drive a save without reaching into the component.

const drafts = new Map<string, string>();
type Listener = (pageId: string, html: string) => void;
const listeners = new Set<Listener>();

/** Keep the newest local body for a page. */
export function setPageBodyDraft(pageId: string, html: string): void {
  drafts.set(pageId, html);
  for (const listener of listeners) listener(pageId, html);
}

/** The newest local body for a page, or null when nothing was typed. */
export function getPageBodyDraft(pageId: string): string | null {
  return drafts.get(pageId) ?? null;
}

/** Drop a draft (a save acknowledged it, or the session ended). */
export function clearPageBodyDraft(pageId: string): void {
  drafts.delete(pageId);
}

/** The sync slice subscribes here to persist as the author types. */
export function subscribePageBodyDraft(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Test reset; drafts must not leak between mounts or suites. */
export function resetPageBodyDrafts(): void {
  drafts.clear();
  listeners.clear();
}
