// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared zod-mini field helpers for contracts.
import * as z from "zod/mini";

/** ISO-8601 datetime as the API serializes it; null when unset. */
export function nullableDateTime() {
  return z.nullable(z.iso.datetime());
}

/** ISO-8601 datetime that may also be absent from the payload. */
export function optionalDateTime() {
  return z.optional(z.nullable(z.iso.datetime()));
}

/** UUID primary key. */
export function uuid() {
  return z.uuid();
}

/** UUID key that may be null (unset foreign key). */
export function nullableUuid() {
  return z.nullable(z.uuid());
}
