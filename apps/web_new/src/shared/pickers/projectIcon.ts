// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mark resolution (parity/specs/issues.md §17, ISS-211). A project
// names its mark either with an emoji or with a Material icon and a color;
// this module reads both shapes from the raw logo value. It deliberately
// knows nothing about the glyph set: the Lucide map lives in
// projectIconGlyphs.tsx, which only loads with the picker's chunk. The
// shell's logo box is eager on every route, and pulling 100+ icon modules
// into that closure is what the initial-JS size gate rejects.

/**
 * A project's mark: an emoji glyph, an uploaded image, an icon name plus
 * color, or the short code's initial. The General tab's emblem control
 * stores an upload as `{ in_use: "upload", url }` and an emoji as
 * `{ in_use: "emoji", emoji: { value } }`, where the value is the emoji's
 * decimal code points (for example "128640" for a rocket).
 */
export type ProjectMark =
  | { kind: "emoji"; glyph: string }
  | { kind: "image"; url: string }
  | { kind: "icon"; name: string; color: string | null }
  | { kind: "initial" };

/**
 * Decode the stored emoji form. A dash-separated decimal list is a set of
 * Unicode code points; anything that does not decode (out of range, not a
 * number) is returned untouched so a literal character still renders.
 */
export function decodeEmojiValue(value: string): string {
  if (!/^\d+(-\d+)*$/.test(value)) return value;
  const points = value.split("-").map((part) => Number(part));
  if (points.some((point) => !isCodePoint(point))) return value;
  try {
    return String.fromCodePoint(...points);
  } catch {
    return value;
  }
}

/** A scalar value: in range and not a lone UTF-16 surrogate. */
function isCodePoint(point: number): boolean {
  return Number.isInteger(point) && point >= 0 && point <= 0x10ffff && (point < 0xd800 || point > 0xdfff);
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** The project's mark with its emoji, icon, or initial fallback. */
export function projectMark(logo: Record<string, unknown>): ProjectMark {
  if (logo.in_use === "upload") {
    const url = logo["url"];
    if (typeof url === "string" && url.trim() !== "") return { kind: "image", url };
  }
  if (logo.in_use === "emoji") {
    const emoji = record(logo.emoji);
    const url = emoji?.["url"];
    const value = emoji?.["value"];
    if (typeof url === "string" && url.trim() !== "") return { kind: "image", url };
    if (typeof value === "string" && value.trim() !== "") {
      const glyph = decodeEmojiValue(value);
      if (glyph.trim() !== "") return { kind: "emoji", glyph };
    }
  }
  if (logo.in_use === "icon") {
    const icon = record(logo.icon);
    const name = icon?.["name"];
    if (typeof name === "string" && name.trim() !== "") {
      const color = icon?.["color"];
      return { kind: "icon", name, color: typeof color === "string" && color.trim() !== "" ? color : null };
    }
  }
  return { kind: "initial" };
}

/** The mark's text form: the emoji glyph, else the short code's initial. */
export function projectMarkText(mark: ProjectMark, identifier: string): string {
  if (mark.kind === "emoji") return mark.glyph;
  const initial = identifier.trim().slice(0, 1).toUpperCase();
  return initial === "" ? "?" : initial;
}
