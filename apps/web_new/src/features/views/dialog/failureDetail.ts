// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Error notice body for a failed view write (parity/specs/views.md §10).
// The server's field detail names the failure when the body carries one;
// anything else falls back to the caller's fixed copy so the notice never
// lands empty.

/** Server detail when the body carries one; the fixed copy otherwise. */
export function failureDetail(error: unknown, fallback: string): string {
  const body = (error as { body?: unknown }).body ?? null;
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    const detail = (body as Record<string, unknown>)["detail"];
    if (typeof detail === "string" && detail.length > 0) return detail;
  }
  return fallback;
}
