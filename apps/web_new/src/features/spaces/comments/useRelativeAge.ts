// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Coarse relative-age clock for public comment rows (SPC-050). The age
// text comes from the shared timeAgo helper; this hook only re-renders it
// on a minute cadence so a visitor watching the thread sees an age move
// without a refetch.

import * as React from "react";

import { timeAgo } from "../../../shared/lib/timeAgo.js";

/** How often a visible relative age is recomputed. */
export const SPACE_AGE_TICK_MS = 60_000;

/**
 * Current relative age of an ISO timestamp. Returns the text and a stable
 * clock reading so callers can format tooltips from the same instant.
 */
export function useRelativeAge(isoTimestamp: string, tickMs: number = SPACE_AGE_TICK_MS): string {
  const [now, setNow] = React.useState(() => Date.now());
  React.useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, tickMs);
    return () => {
      clearInterval(timer);
    };
  }, [tickMs]);
  return timeAgo(isoTimestamp, now);
}
