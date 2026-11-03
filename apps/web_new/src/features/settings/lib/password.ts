// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Security-tab password rules (parity/specs/profile.md §6, PROF-015).
// The client gates submit on a strong secret, a matching confirmation and
// a guess different from the current one; the mismatch note waits until
// the confirmation blurs or grows at least as long as the guess; the
// reveal toggles are per field. The server stays authoritative and its
// error codes are mapped here.

/** Shortest secret the meter accepts. */
export const PASSWORD_MIN_LENGTH = 8;

/** The rules the meter lists beside a weak guess. */
export const PASSWORD_RULES: readonly { id: "length" | "upper" | "lower" | "digit" | "symbol"; label: string }[] = [
  { id: "length", label: "Min 8 characters" },
  { id: "upper", label: "An upper-case letter" },
  { id: "lower", label: "A lower-case letter" },
  { id: "digit", label: "A number" },
  { id: "symbol", label: "A symbol" },
];

export interface PasswordRuleState {
  id: (typeof PASSWORD_RULES)[number]["id"];
  label: string;
  met: boolean;
}

/** Which rules a guess meets, for the strength meter. */
export function passwordRules(guess: string): PasswordRuleState[] {
  return PASSWORD_RULES.map((rule) => {
    let met = false;
    switch (rule.id) {
      case "length":
        met = guess.length >= PASSWORD_MIN_LENGTH;
        break;
      case "upper":
        met = /[A-Z]/.test(guess);
        break;
      case "lower":
        met = /[a-z]/.test(guess);
        break;
      case "digit":
        met = /[0-9]/.test(guess);
        break;
      case "symbol":
        met = /[^A-Za-z0-9]/.test(guess);
        break;
    }
    return { ...rule, met };
  });
}

/** True once every rule holds. */
export function isStrongPassword(guess: string): boolean {
  return passwordRules(guess).every((rule) => rule.met);
}

export type PasswordField = "current" | "next" | "confirm";

export interface PasswordFormValues {
  current: string;
  next: string;
  confirm: string;
}

export interface PasswordFormState {
  /** The current-password field is offered only to accounts that set one. */
  requiresCurrent: boolean;
  focused: PasswordField | null;
  confirmTouched: boolean;
  values: PasswordFormValues;
}

export const PASSWORD_MESSAGES = {
  mismatch: "Passwords don't match",
  sameAsOld: "New password must be different from old password",
  weakFromServer: "Please use a stronger password.",
  currentRequired: "Enter your current password",
  newRequired: "Enter a new password",
} as const;

/** The mismatch note: hidden while a shorter guess still has focus. */
export function mismatchVisible(state: PasswordFormState): boolean {
  const { next, confirm } = state.values;
  if (confirm === "" || confirm === next) return false;
  return state.confirmTouched || confirm.length >= next.length;
}

/** The same-as-old note: hidden while the new field still has focus. */
export function sameAsOldVisible(state: PasswordFormState): boolean {
  const { current, next } = state.values;
  if (current === "" || next === "") return false;
  return current === next && state.focused !== "next";
}

/** Whether the strength meter shows: a non-empty guess that is not yet strong. */
export function strengthVisible(guess: string): boolean {
  return guess.length > 0 && !isStrongPassword(guess);
}

/** Per-field client errors for an attempted submit. */
export function passwordErrors(state: PasswordFormState): Partial<Record<"current" | "new", string>> {
  const errors: Partial<Record<"current" | "new", string>> = {};
  if (state.requiresCurrent && state.values.current.trim() === "") {
    errors.current = PASSWORD_MESSAGES.currentRequired;
  }
  if (state.values.next.trim() === "") errors.new = PASSWORD_MESSAGES.newRequired;
  return errors;
}

/** Submit gate: field presence, strength, match and difference from the old secret. */
export function canSubmitPassword(state: PasswordFormState): boolean {
  const { current, next, confirm } = state.values;
  if (Object.keys(passwordErrors(state)).length > 0) return false;
  if (!isStrongPassword(next)) return false;
  if (next !== confirm) return false;
  return !(state.requiresCurrent && current !== "" && current === next);
}

/** Server error codes the password write can answer. */
export const PASSWORD_ERROR_CODES = {
  incorrectOld: "5135",
  missing: "5138",
  tooWeak: "5021",
} as const;

export interface PasswordFailure {
  /** Toast title shown for the failure. */
  title: string;
  message: string;
  /** Field that carries an inline message, when one does. */
  field: "current" | "new" | null;
}

/**
 * Map a failed password write to its feedback. The server answers
 * `{error_code, error_message, error}`; an unknown failure falls back to
 * its message, or a generic line when none is usable.
 */
export function describePasswordFailure(error: unknown): PasswordFailure {
  const record = (error ?? {}) as { code?: unknown; message?: unknown };
  const code = typeof record.code === "number" || typeof record.code === "string" ? String(record.code) : "";
  if (code === PASSWORD_ERROR_CODES.incorrectOld) {
    return {
      title: "Incorrect old password",
      message: "Incorrect old password. Please try again.",
      field: null,
    };
  }
  if (code === PASSWORD_ERROR_CODES.tooWeak) {
    return {
      title: "Password too weak",
      message: PASSWORD_MESSAGES.weakFromServer,
      field: "new",
    };
  }
  if (code === PASSWORD_ERROR_CODES.missing) {
    return {
      title: "Password missing",
      message: "Enter your current and new password.",
      field: "current",
    };
  }
  const message = typeof record.message === "string" && record.message.length > 0 ? record.message : null;
  return {
    title: "Could not change password",
    message: message ?? "Something went wrong. Please try again.",
    field: null,
  };
}

/**
 * Which field carries the inline message for a server refusal: a weak
 * guess maps to the new password, a missing secret to the current one.
 */
export function classifyPasswordFieldError(error: unknown): "current" | "new" | null {
  const record = (error ?? {}) as { code?: unknown };
  const code = record.code === undefined || record.code === null ? "" : String(record.code);
  if (code === PASSWORD_ERROR_CODES.tooWeak) return "new";
  if (code === PASSWORD_ERROR_CODES.missing) return "current";
  return null;
}
