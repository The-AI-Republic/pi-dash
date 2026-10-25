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

/** The collaboration layer reports connection state through this seam. */
export function reportPageSync(pageId: string, status: PageSyncStatus): void {
  for (const listener of listeners) listener(pageId, status);
}

/**
 * Page-scoped status: a fresh page starts as `syncing`; with no live
 * connection reporting in, it settles to `lost` after the short beat.
 */
export function usePageSyncStatus(pageId: string): PageSyncStatus {
  const [status, setStatus] = React.useState<PageSyncStatus>("syncing");
  const [trackedPage, setTrackedPage] = React.useState(pageId);
  if (trackedPage !== pageId) {
    setTrackedPage(pageId);
    setStatus("syncing");
  }
  React.useEffect(() => {
    const onReport: Listener = (reportedPage, reportedStatus) => {
      if (reportedPage === pageId) setStatus(reportedStatus);
    };
    listeners.add(onReport);
    const timer = setTimeout(() => setStatus("lost"), PAGE_SYNC_SETTLE_MS);
    return () => {
      clearTimeout(timer);
      listeners.delete(onReport);
    };
  }, [pageId]);
  return status;
}
