// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Incremental history reads (parity/specs/comments.md §4.5, CMT-017).
// After the first load each feed source refetches only what is new: the
// query carries `created_at__gt` set to the newest creation timestamp the
// cache already holds for that source, and the returned rows join the
// cached list in place. Each source keeps its own cursor, so a comment's
// timestamp never gates a property read.

/** Rows the delta merge understands: an id plus a creation timestamp. */
export interface TimestampedRow {
  readonly id: string;
  readonly created_at: string;
}

/**
 * The newest known creation timestamp, the `created_at__gt` cursor for
 * the next read. Returns the stored string untouched (the server
 * compares it as given). Empty lists and unreadable timestamps yield no
 * cursor, and the caller falls back to a full read.
 */
export function newestCreatedAt(rows: readonly TimestampedRow[]): string | undefined {
  let newest: string | undefined;
  let newestMs = 0;
  for (const row of rows) {
    const ms = Date.parse(row.created_at);
    if (Number.isNaN(ms)) continue;
    if (newest === undefined || ms > newestMs) {
      newest = row.created_at;
      newestMs = ms;
    }
  }
  return newest;
}

/**
 * Join a delta onto the cached rows, skipping ids already present so a
 * repeated row (clock skew, overlapping refetches) never duplicates.
 * Appends in delta order; the caller keeps its own ordering.
 */
export function mergeDeltaRows<T extends TimestampedRow>(previous: readonly T[], delta: readonly T[]): T[] {
  const seen = new Set(previous.map((row) => row.id));
  const merged = [...previous];
  for (const row of delta) {
    if (!seen.has(row.id)) {
      seen.add(row.id);
      merged.push(row);
    }
  }
  return merged;
}
