// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view title validation (parity/specs/views.md §5, VIEW-013). The
// create dialog (and later the edit dialog) validates locally before it
// submits: a title is required and capped at 255 characters.

/** Longest title the server accepts on a saved view. */
export const VIEW_TITLE_MAX_LENGTH = 255;

/**
 * Inline error for a dialog title, or null when it submits. A blank title
 * (nothing but whitespace) is missing; anything past the cap names it.
 */
export function validateViewTitle(title: string): string | null {
  if (title.trim() === "") return "Title is required";
  if (title.length > VIEW_TITLE_MAX_LENGTH) return `Title must be at most ${VIEW_TITLE_MAX_LENGTH} characters`;
  return null;
}
