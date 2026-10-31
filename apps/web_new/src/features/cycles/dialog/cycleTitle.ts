// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle title validation (parity/specs/cycles.md §6, CYC-015). The
// create/edit dialog (and later the sidebar inline editing in slice G)
// validates locally before it submits: a title is required and capped
// at 255 characters.

/** Longest title the server accepts on a cycle. */
export const CYCLE_TITLE_MAX_LENGTH = 255;

/**
 * Inline error for a dialog title, or null when it submits. A blank title
 * (nothing but whitespace) is missing; anything past the cap names it.
 */
export function validateCycleTitle(title: string): string | null {
  if (title.trim() === "") return "Title is required";
  if (title.length > CYCLE_TITLE_MAX_LENGTH) return `Title must be at most ${CYCLE_TITLE_MAX_LENGTH} characters`;
  return null;
}
