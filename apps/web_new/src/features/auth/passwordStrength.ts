// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-password strength gate (AUTH-010). The sign-up card refuses weak
// secrets before anything is posted; the same rule drives the strength
// hint beside the secret field. The gates (banner, mismatch block, hint)
// come from parity/specs/auth.md AUTH-010; the acceptance set below
// mirrors the old app's observed rule — eight characters plus one of
// each class, ASCII letters only, and a fixed punctuation set — so edge
// secrets classify the same here.

const MIN_SECRET_LENGTH = 8;

const ASCII_UPPER_FIRST = 65;
const ASCII_UPPER_LAST = 90;
const ASCII_LOWER_FIRST = 97;
const ASCII_LOWER_LAST = 122;
const ASCII_DIGIT_FIRST = 48;
const ASCII_DIGIT_LAST = 57;

/**
 * Whether a code unit belongs to the fixed punctuation set: the `!`
 * through `/` and `:` through `@` runs, the four bracket marks
 * `[ ] ^ _`, and the `{ | }` run. Anything else — spaces, backslashes,
 * backticks, tildes, non-ASCII — does not satisfy the rule.
 */
function isAcceptedMark(code: number): boolean {
  if (code >= 33 && code <= 47) return true;
  if (code >= 58 && code <= 64) return true;
  if (code === 91 || code === 93 || code === 94 || code === 95) return true;
  return code >= 123 && code <= 125;
}

function hasUpperCase(value: string): boolean {
  for (const char of value) {
    const code = char.charCodeAt(0);
    if (code >= ASCII_UPPER_FIRST && code <= ASCII_UPPER_LAST) return true;
  }
  return false;
}

function hasLowerCase(value: string): boolean {
  for (const char of value) {
    const code = char.charCodeAt(0);
    if (code >= ASCII_LOWER_FIRST && code <= ASCII_LOWER_LAST) return true;
  }
  return false;
}

function hasDigit(value: string): boolean {
  for (const char of value) {
    const code = char.charCodeAt(0);
    if (code >= ASCII_DIGIT_FIRST && code <= ASCII_DIGIT_LAST) return true;
  }
  return false;
}

/** True once any character belongs to the fixed punctuation set. */
function hasPunctuation(value: string): boolean {
  for (const char of value) {
    if (isAcceptedMark(char.charCodeAt(0))) return true;
  }
  return false;
}

export interface SecretCriterion {
  id: "length" | "upper" | "lower" | "digit" | "punctuation";
  hint: string;
  met: boolean;
}

/** Per-rule breakdown for the strength hint; empty values meet nothing. */
export function secretCriteria(value: string): SecretCriterion[] {
  return [
    { id: "length", hint: "At least 8 characters", met: value.length >= MIN_SECRET_LENGTH },
    { id: "upper", hint: "An upper-case letter", met: hasUpperCase(value) },
    { id: "lower", hint: "A lower-case letter", met: hasLowerCase(value) },
    { id: "digit", hint: "A number", met: hasDigit(value) },
    { id: "punctuation", hint: "A special character", met: hasPunctuation(value) },
  ];
}

/** True once every strength rule holds. */
export function isStrongSecret(value: string): boolean {
  return secretCriteria(value).every((criterion) => criterion.met);
}
