// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Browser connectivity for the pages detail (parity/specs/pages.md §8.3,
// PAGE-062). The offline pill reads this hook: it appears a short beat
// after the browser reports no connectivity on an editable page, and
// clears the same beat after the browser comes back, so a flapping
// connection never flickers the header.

import * as React from "react";

/** The pill's pinned word. */
export const PAGE_OFFLINE_LABEL = "Offline";

/** How long a connectivity change settles before the pill follows it. */
export const PAGE_OFFLINE_BEAT_MS = 400;

/** True while the browser reports an online connection. */
export function useBrowserOnline(): boolean {
  const [online, setOnline] = React.useState(() => navigator.onLine);
  React.useEffect(() => {
    const reportOnline = (): void => setOnline(true);
    const reportOffline = (): void => setOnline(false);
    window.addEventListener("online", reportOnline);
    window.addEventListener("offline", reportOffline);
    return () => {
      window.removeEventListener("online", reportOnline);
      window.removeEventListener("offline", reportOffline);
    };
  }, []);
  return online;
}

/**
 * Whether the offline pill shows: an editable page whose browser reports
 * offline, delayed by the settle beat in both directions.
 */
export function usePageOfflineVisible(editable: boolean): boolean {
  const online = useBrowserOnline();
  const [visible, setVisible] = React.useState(false);
  React.useEffect(() => {
    const target = editable && !online;
    if (target === visible) return;
    const timer = setTimeout(() => setVisible(target), PAGE_OFFLINE_BEAT_MS);
    return () => clearTimeout(timer);
  }, [editable, online, visible]);
  return visible;
}
