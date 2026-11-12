// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General-tab name validation (parity/specs/profile.md §6, PROF-011).
// First and display names are required; the last name is optional and may
// be cleared (the target for NEWFRONT-432). Person names allow letters,
// spaces, hyphens and apostrophes; display names also allow digits,
// periods and underscores. All three cap at fifty characters.

export const PERSON_NAME_MAX_LENGTH = 50;
export const DISPLAY_NAME_MAX_LENGTH = 50;

const PERSON_NAME_PATTERN = /^[\p{L} '-]+$/u;
const DISPLAY_NAME_PATTERN = /^[\p{L}\p{N}._-]+$/u;

export type NameField = "first_name" | "last_name" | "display_name";

export type NameErrors = Partial<Record<NameField, string>>;

export const NAME_MESSAGES = {
  firstRequired: "Please enter first name",
  displayRequired: "Please enter display name",
  personCharacters: "Names can only contain letters, spaces, hyphens, and apostrophes",
  displayCharacters: "Display name can only contain letters, numbers, periods, hyphens, and underscores",
  tooLong: `Keep this to ${PERSON_NAME_MAX_LENGTH} characters or fewer`,
} as const;

function personNameError(value: string, required: string | null): string | null {
  if (value.length === 0) return required;
  if (value.length > PERSON_NAME_MAX_LENGTH) return NAME_MESSAGES.tooLong;
  return PERSON_NAME_PATTERN.test(value) ? null : NAME_MESSAGES.personCharacters;
}

function displayNameError(value: string): string | null {
  if (value.length === 0) return NAME_MESSAGES.displayRequired;
  if (value.length > DISPLAY_NAME_MAX_LENGTH) return NAME_MESSAGES.tooLong;
  return DISPLAY_NAME_PATTERN.test(value) ? null : NAME_MESSAGES.displayCharacters;
}

/** Per-field inline errors for the general form; empty when every field is valid. */
export function validateNames(values: Record<NameField, string>): NameErrors {
  const errors: NameErrors = {};
  const first = personNameError(values.first_name, NAME_MESSAGES.firstRequired);
  if (first !== null) errors.first_name = first;
  const last = personNameError(values.last_name, null);
  if (last !== null) errors.last_name = last;
  const display = displayNameError(values.display_name);
  if (display !== null) errors.display_name = display;
  return errors;
}

/** True when no field carries an error. */
export function namesAreValid(values: Record<NameField, string>): boolean {
  return Object.keys(validateNames(values)).length === 0;
}
