// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth entry search primitives. The sign-in entry (/) and the sign-up
// card (/sign-up) share these: entries are lenient (unknown or invalid
// params fall back to absent) and return paths stay internal-only.

/** First non-empty string, else absent. */
export function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/** Internal return paths only; anything else falls back to post-auth landing. */
export function safeNextPath(value: unknown): string | undefined {
  const path = asString(value);
  if (!path || !path.startsWith("/") || path.startsWith("//")) return undefined;
  return path;
}
