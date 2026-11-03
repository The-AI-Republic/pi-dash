// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// State form rules (parity/specs/project-settings-states.md, PSET-029).
// The name is required after trimming and capped at a hundred characters
// by the input itself; the description is free text. Duplicate and triage
// refusals are the server's to make, so the client only shapes input and
// explains a refusal afterwards.

/** Longest state name the create/edit form accepts. */
export const STATE_NAME_MAX_LENGTH = 100;

/** Inline error for a blank name. */
export const STATE_NAME_REQUIRED = "State name is required";

/** Clamp a typed name to the field's cap (paste and IME included). */
export function capStateName(value: string): string {
  return value.slice(0, STATE_NAME_MAX_LENGTH);
}

/**
 * Name problem to show inline, or null when the value may be sent. The
 * empty check runs on the trimmed value so whitespace never becomes a
 * state; the untrimmed name is what the caller submits, matching what
 * the operator typed.
 */
export function validateStateName(value: string): string | null {
  return value.trim() === "" ? STATE_NAME_REQUIRED : null;
}
