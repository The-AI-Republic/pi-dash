// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mark decoding for the public navbar. The publish payload carries
// a loose logo map; a malformed emoji is a payload fault, so it throws and
// the space error boundary renders the crash screen (SPC-019).

export function projectMarkGlyph(logoProps: Record<string, unknown>): string | null {
  if (logoProps["in_use"] !== "emoji") return null;
  const emoji = logoProps["emoji"];
  if (typeof emoji !== "object" || emoji === null) return null;
  const value = (emoji as Record<string, unknown>)["value"];
  if (typeof value !== "string" && typeof value !== "number") return null;
  const codePoint = Number(value);
  if (!Number.isInteger(codePoint) || codePoint < 0 || codePoint > 0x10ffff) {
    throw new RangeError(`Invalid project emoji code point: ${String(value)}`);
  }
  return String.fromCodePoint(codePoint);
}
