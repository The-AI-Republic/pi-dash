// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sync indicator state (parity/specs/pages.md §8.2, PAGE-024).
// The badge reads a small store rather than the socket directly, so the
// collaboration slice can drive it without the shell changing: a switch to
// another page starts a transitional beat, and with no live connection the
// settled state is the stable fault badge. Normal editing never shows one.

import * as React from "react";

export type PageSyncStatus = "syncing" | "synced" | "lost";

/** Wording the oracle pins on the header cluster. */
export const PAGE_SYNC_SYNCING = "Syncing...";
export const PAGE_SYNC_LOST = "Connection lost";

/** How long a page switch shows the transitional state before it settles. */
export const PAGE_SYNC_SETTLE_MS = 400;

type Listener = (pageId: string, status: PageSyncStatus) => void;

const listeners = new Set<Listener>();

/**
 * The collaboration layer reports connection state through this seam:
 * connecting, awaiting-sync and reconnecting all report `syncing`, a
 * converged document reports `synced`, and the terminal disconnected
 * state reports `lost` (spec §8.2, §8.4; PAGE-061, PAGE-066, PAGE-067).
 */
export function reportPageSync(pageId: string, status: PageSyncStatus): void {
  for (const listener of listeners) listener(pageId, status);
}

/**
 * Page-scoped status: a fresh page starts as `syncing`; with no live
 * connection reporting in, it settles to `lost` after the short beat. A
 * report stands: once the collaboration layer speaks for this page, the
 * settle beat no longer clobbers the reported state.
 */
export function usePageSyncStatus(pageId: string): PageSyncStatus {
  const [status, setStatus] = React.useState<PageSyncStatus>("syncing");
  const [trackedPage, setTrackedPage] = React.useState(pageId);
  if (trackedPage !== pageId) {
    setTrackedPage(pageId);
    setStatus("syncing");
  }
  React.useEffect(() => {
    let reported = false;
    const onReport: Listener = (reportedPage, reportedStatus) => {
      if (reportedPage !== pageId) return;
      reported = true;
      setStatus(reportedStatus);
    };
    listeners.add(onReport);
    const timer = setTimeout(() => {
      if (!reported) setStatus("lost");
    }, PAGE_SYNC_SETTLE_MS);
    return () => {
      clearTimeout(timer);
      listeners.delete(onReport);
    };
  }, [pageId]);
  return status;
}
