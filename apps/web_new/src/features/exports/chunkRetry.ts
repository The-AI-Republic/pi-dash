// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bounded retry for lazy chunk loads (NEWFRONT-772, EXP-023). The
// settings exports route lazy-loads the guide chunk so it stays out of
// the initial bundle; a transient load failure must retry instead of
// pinning the "guide unavailable" fallback, while a persistently broken
// chunk still surfaces after the last attempt.

/** How many times a lazy chunk load is attempted before giving up. */
export const CHUNK_LOAD_ATTEMPTS = 4;

/**
 * Runs `load` until it resolves, up to `attempts` tries. Resolves on the
 * first success; when every attempt rejects, rejects with the last error.
 */
export async function loadWithRetry(load: () => Promise<unknown>, attempts: number): Promise<void> {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      await load();
      return;
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}
