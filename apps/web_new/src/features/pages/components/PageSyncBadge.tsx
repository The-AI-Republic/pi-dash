// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Header sync badge (parity/specs/pages.md §8.2, PAGE-024, PAGE-061).
// Transitional and fault states carry a word; the synced state renders
// nothing, so a healthy page never keeps a badge on screen. The last word
// lingers a short beat after a synced report before hiding, and the fault
// note states the real background-save cadence.

import { CloudOff, RefreshCw } from "lucide-react";
import * as React from "react";

import { PAGE_SYNC_LOST, PAGE_SYNC_SYNCING, type PageSyncStatus } from "../lib/pageSync.js";

export const PAGE_SYNC_LOST_HINT = "Connection lost. Changes keep saving in the background every 30 seconds.";
/** How long the last word lingers after a synced report before hiding. */
export const PAGE_SYNC_HIDE_MS = 300;

export interface PageSyncBadgeProps {
  status: PageSyncStatus;
}

export function PageSyncBadge({ status }: PageSyncBadgeProps): React.ReactElement | null {
  const [shown, setShown] = React.useState<PageSyncStatus>(status);
  React.useEffect(() => {
    if (status !== "synced") {
      setShown(status);
      return;
    }
    if (shown === "synced") return;
    const timer = setTimeout(() => setShown("synced"), PAGE_SYNC_HIDE_MS);
    return () => clearTimeout(timer);
  }, [status, shown]);
  if (shown === "synced") return null;
  if (shown === "syncing") {
    return (
      <span className="text-caption inline-flex items-center gap-(--space-1) text-(--text-muted)">
        <RefreshCw size={12} strokeWidth={1.5} aria-hidden="true" />
        {PAGE_SYNC_SYNCING}
      </span>
    );
  }
  return (
    <span
      title={PAGE_SYNC_LOST_HINT}
      className="text-caption text-(--warning, var(--text-muted)) inline-flex items-center gap-(--space-1)"
    >
      <CloudOff size={12} strokeWidth={1.5} aria-hidden="true" />
      {PAGE_SYNC_LOST}
    </span>
  );
}
