// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-password strength gate (AUTH-010). The sign-up card refuses weak
// secrets before anything is posted; the same rule drives the strength
// hint beside the secret field. Written from parity/specs/auth.md
// AUTH-010: a strong secret is at least eight characters long and mixes
// upper-case, lower-case, digits, and punctuation.

const MIN_SECRET_LENGTH = 8;

/** Non-alphanumeric characters count as the required punctuation. */
function hasPunctuation(value: string): boolean {
  for (const char of value) {
    const code = char.charCodeAt(0);
    const alphanumeric = (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
    if (!alphanumeric) return true;
  }
  return false;
}

function hasUpperCase(value: string): boolean {
  return value !== value.toLowerCase();
}

function hasLowerCase(value: string): boolean {
  return value !== value.toUpperCase();
}

function hasDigit(value: string): boolean {
  for (const char of value) {
    if (char >= "0" && char <= "9") return true;
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
