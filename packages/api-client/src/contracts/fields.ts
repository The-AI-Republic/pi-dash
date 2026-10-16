// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared zod-mini field helpers for contracts.
import * as z from "zod/mini";

/**
 * ISO-8601 datetime as the API serializes it. Django renders datetimes
 * in the caller's timezone, so non-UTC users get numeric offsets
 * (2026-10-16T15:32:05.479661+09:00); the bare Z-only pattern rejects
 * those and must never be used for API datetimes directly.
 */
export function dateTime() {
  return z.iso.datetime({ offset: true });
}

/** ISO-8601 calendar date as the API serializes it; null when unset. */
export function nullableDate() {
  return z.nullable(z.iso.date());
}

/** ISO-8601 datetime as the API serializes it; null when unset. */
export function nullableDateTime() {
  return z.nullable(dateTime());
}

/** ISO-8601 datetime that may also be absent from the payload. */
export function optionalDateTime() {
  return z.optional(z.nullable(dateTime()));
}

/** UUID primary key. */
export function uuid() {
  return z.uuid();
}

/** UUID key that may be null (unset foreign key). */
export function nullableUuid() {
  return z.nullable(z.uuid());
}
