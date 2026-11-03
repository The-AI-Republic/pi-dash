// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bounded retry for lazy chunk loads (NEWFRONT-772, EXP-023/024). The
// settings exports route lazy-loads the guide chunk so it stays out of
// the initial bundle; a transient load failure must retry across the
// same window the driver waits for the guide instead of pinning the
// "guide unavailable" fallback, while a persistently broken chunk still
// surfaces after the last attempt.

/** How many times a lazy chunk load is attempted before giving up. */
export const CHUNK_LOAD_ATTEMPTS = 10;
/** Delay between chunk-load attempts, in milliseconds. */
export const CHUNK_LOAD_RETRY_DELAY_MS = 3000;

/**
 * Runs `load` until it resolves, up to `attempts` tries spaced `delayMs`
 * apart (the first attempt runs immediately). Resolves on the first
 * success; when every attempt rejects, rejects with the last error.
 */
export async function loadWithRetry(load: () => Promise<unknown>, attempts: number, delayMs: number): Promise<void> {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
    try {
      await load();
      return;
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}
