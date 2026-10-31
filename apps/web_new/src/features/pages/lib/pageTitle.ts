// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title rules (parity/specs/pages.md §5.1, PAGE-026). The title is one
// line of text capped at 255 characters: input past the cap is refused
// rather than stored, pasted text loses its line breaks, and the count
// the editor shows is the length of what would actually save. Everything
// a caller needs to decide those rules lives here, without an editor.

import { PAGE_TITLE_MAX_LENGTH } from "./pageCreate.js";
import { PAGE_UNTITLED } from "./pageList.js";

/** Longest title the detail editor accepts (spec §5.1), one cap for both. */
export { PAGE_TITLE_MAX_LENGTH };

/** Placeholder the empty heading shows, on the editable and static faces. */
export const PAGE_TITLE_PLACEHOLDER = PAGE_UNTITLED;

/** A title draft clipped to the cap; the stored value may be longer. */
export function clipTitle(text: string): string {
  return text.length > PAGE_TITLE_MAX_LENGTH ? text.slice(0, PAGE_TITLE_MAX_LENGTH) : text;
}

/** Room left in a title that already holds `length` characters. */
export function titleRoom(length: number): number {
  return Math.max(0, PAGE_TITLE_MAX_LENGTH - length);
}

/**
 * Pasted title text in the shape the heading can hold: line breaks and
 * other whitespace runs collapse to one space, so a pasted paragraph
 * never splits the single-line title.
 */
export function pasteTitleText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * The text a paste event carries for the title: the plain-text flavor,
 * with a paste that has none reading as blank and so refused.
 */
export function pastedTitleOf(text: string | undefined): string {
  return pasteTitleText(text ?? "");
}

/** Characters a title save would carry: the clipped, trimmed text. */
export function titleLength(text: string): number {
  return clipTitle(text).length;
}

/** Whether a saved title reads as blank (an empty heading). */
export function titleIsEmpty(text: string): boolean {
  return text.trim() === "";
}

/**
 * How many characters a one-line text must lose from its end to fit the
 * cap. The count badge reads the same limit, so the two never disagree.
 */
export function titleOverflow(text: string): number {
  return Math.max(0, text.length - PAGE_TITLE_MAX_LENGTH);
}
