// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A trailing-edge debounce for scroll triggers: a burst of scroll events
// collapses into one call after the burst goes quiet, so one page is
// requested per gesture rather than per event.

export interface Debounced<Args extends unknown[]> {
  (...args: Args): void;
  /** Drop a pending call and stop listening. */
  cancel: () => void;
}

export function debounce<Args extends unknown[]>(fn: (...args: Args) => void, waitMs: number): Debounced<Args> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const debounced = (...args: Args): void => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      fn(...args);
    }, waitMs);
  };
  debounced.cancel = (): void => {
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
  };
  return debounced;
}
