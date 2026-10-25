// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Header sync badge (parity/specs/pages.md §8.2, PAGE-024). Transitional
// and fault states carry a word; the synced state renders nothing, so a
// healthy page never keeps a badge on screen.

import { CloudOff, RefreshCw } from "lucide-react";
import * as React from "react";

import { PAGE_SYNC_LOST, PAGE_SYNC_SYNCING, type PageSyncStatus } from "../lib/pageSync.js";

export const PAGE_SYNC_LOST_HINT = "Saves continue in the background while the connection is down.";

export interface PageSyncBadgeProps {
  status: PageSyncStatus;
}

export function PageSyncBadge({ status }: PageSyncBadgeProps): React.ReactElement | null {
  if (status === "synced") return null;
  if (status === "syncing") {
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
