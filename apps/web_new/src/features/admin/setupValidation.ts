// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup-form validation rules (ADM-005, from parity/specs/admin.md
// §1.5). Pure predicates: the form swallows rejected keystrokes, the
// submit gate, and the meter/mismatch visibility rules. Strength
// itself reuses the shared auth rule.

import { isStrongSecret } from "../auth/index.js";

export const PERSON_NAME_MAX_LENGTH = 50;
export const COMPANY_NAME_MAX_LENGTH = 80;

/** Person names: letters, spaces, hyphens, apostrophes. */
const PERSON_NAME_PATTERN = /^[\p{L}\s'-]+$/u;

/** Company names: letters, numbers, spaces, hyphens, underscores. */
const COMPANY_NAME_PATTERN = /^[\p{L}\p{N}\s_-]+$/u;

export function isPersonNameText(value: string): boolean {
  // Blank values never validate (the old form's required check rejects
  // them too), so a whitespace-only keystroke is swallowed; clearing
  // still goes through acceptPersonName's explicit empty bypass.
  return (
    value.length > 0 &&
    value.length <= PERSON_NAME_MAX_LENGTH &&
    value.trim().length > 0 &&
    PERSON_NAME_PATTERN.test(value)
  );
}

export function isCompanyNameText(value: string): boolean {
  return value.length > 0 && value.length <= COMPANY_NAME_MAX_LENGTH && COMPANY_NAME_PATTERN.test(value);
}

/**
 * Whole-value accept for name keystrokes: empty clears the field,
 * otherwise the value must validate, so rejected input leaves the
 * previous value untouched.
 */
export function acceptPersonName(value: string): boolean {
  return value === "" || isPersonNameText(value);
}

/** Whole-value accept for company keystrokes; same swallow rule. */
export function acceptCompanyName(value: string): boolean {
  return value === "" || isCompanyNameText(value);
}

export interface SetupGateInput {
  firstName: string;
  email: string;
  password: string;
  confirmPassword: string;
  /** The hidden CSRF token is loaded; without it no POST can succeed. */
  csrfReady: boolean;
}

/**
 * Submit opens once first name, email, and password are non-empty, the
 * password is strong, and the confirmation matches. Last name and
 * company never gate.
 */
export function isSetupSubmittable(input: SetupGateInput): boolean {
  return (
    input.csrfReady &&
    input.firstName.length > 0 &&
    input.email.length > 0 &&
    input.password.length > 0 &&
    isStrongSecret(input.password) &&
    input.password === input.confirmPassword
  );
}

/**
 * The meter shows unless the field is blurred with a strong secret, so
 * an empty untouched field still shows its (messageless) meter.
 */
export function isStrengthMeterVisible(password: string, focused: boolean): boolean {
  return focused || !isStrongSecret(password);
}

export interface MismatchInput {
  password: string;
  confirmPassword: string;
  confirmFocused: boolean;
}

/**
 * The mismatch hint shows once the confirmation is non-empty and
 * differs, but stays hidden while the confirmation still has focus
 * and trails the password's length.
 */
export function isMismatchVisible(input: MismatchInput): boolean {
  if (input.confirmPassword.length === 0 || input.confirmPassword === input.password) return false;
  return !input.confirmFocused || input.confirmPassword.length >= input.password.length;
}
