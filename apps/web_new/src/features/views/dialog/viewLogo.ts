// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored logo values for the view dialog (parity/specs/views.md §6,
// VIEW-015). The edit dialog prefills its icon picker from the
// record's `logo_props`; whatever does not match a mark the picker can
// show reads as no logo rather than a fabricated one.

import type { ViewLogoProps } from "./ViewIconPicker.js";

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * The picker's value for a stored `logo_props`, or null when the record
 * carries no logo or one this client cannot show (an unknown `in_use`,
 * a missing payload, a non-string emoji value or glyph name/color).
 */
export function parseViewLogo(logoProps: unknown): ViewLogoProps | null {
  const stored = record(logoProps);
  if (stored === null) return null;
  if (stored["in_use"] === "emoji") {
    const emoji = record(stored["emoji"]);
    const value = emoji?.["value"];
    return typeof value === "string" && value !== "" ? { in_use: "emoji", emoji: { value } } : null;
  }
  if (stored["in_use"] === "icon") {
    const icon = record(stored["icon"]);
    const name = icon?.["name"];
    const color = icon?.["color"];
    return typeof name === "string" && name !== "" && typeof color === "string" && color !== ""
      ? { in_use: "icon", icon: { name, color } }
      : null;
  }
  return null;
}
