// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
