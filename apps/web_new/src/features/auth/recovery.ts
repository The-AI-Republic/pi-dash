// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recovery logic. Pure mappings for the forgot/reset/set-password pages:
// failure-to-message rules, the resend countdown, and the AUTH-010 secret
// gates (strength plus confirmation match). Written from
// parity/specs/auth.md AUTH-010 and AUTH-012-014; the token-banner phrases
// match the oracle pins in e2e/parity/auth/sign-up-recovery.spec.ts.

import { isApiError } from "@pidash/api-client";

import { describeAuthError } from "./authErrors.js";

/** Resend throttle on the forgot-password page (AUTH-012). */
export const FORGOT_RESEND_COOLDOWN_S = 30;

/** Minimum secret length on the reset form (AUTH-010). */
export const MIN_SECRET_LENGTH = 8;

/** Success toast title on the forgot-password page. */
export const FORGOT_SUCCESS_TITLE = "Check your inbox";

/** Failure toast title on the forgot-password page. */
export const FORGOT_FAILURE_TITLE = "Could not send a reset link";

/** Failure toast title on the set-password page. */
export const SET_FAILURE_TITLE = "Could not set a password";

/** Banner when the reset link is invalid (oracle-pinned phrasing). */
export const INVALID_TOKEN_BANNER = "Invalid password token. Request a fresh reset link.";

/** Banner when the reset link has expired (oracle-pinned phrasing). */
export const EXPIRED_TOKEN_BANNER = "Expired password token. Request a fresh reset link.";

/** Fallback when a reset submit fails with no usable server detail. */
export const RESET_FALLBACK_MESSAGE = "Could not reset your password. Check your connection and try again.";

/** Fallback when a set-password submit fails with no usable server detail. */
export const SET_FALLBACK_MESSAGE = "Could not set your password. Try again in a moment.";

/** Fallback when a forgot-password submit fails with no usable server detail. */
export const FORGOT_FALLBACK_MESSAGE = "Try again in a moment.";

/** Server code for an address with no account (forgot-password must not leak it). */
const UNKNOWN_ACCOUNT_CODE = "5060";
const UNKNOWN_ACCOUNT_MESSAGE = "USER_DOES_NOT_EXIST";

/** Server codes for a dead reset link. */
const INVALID_TOKEN_CODE = "5125";
const EXPIRED_TOKEN_CODE = "5130";

/**
 * Context-specific copies where the card matrix message fits another
 * surface: the 5025 message talks about code login, which is wrong on the
 * forgot-password page.
 */
const FORGOT_MESSAGE_OVERRIDES: Record<string, string> = {
  "5025": "This server cannot send email. Contact your administrator.",
};

/** Success toast body on the forgot-password page. */
export function forgotSuccessDescription(email: string): string {
  return `A password reset link is on its way to ${email}.`;
}

/** Seconds left on a resend countdown (0 when idle or elapsed). */
export function resendCountdown(lastSentAt: number | null, now: number, cooldownS: number): number {
  if (lastSentAt === null) return 0;
  return Math.max(0, cooldownS - Math.floor((now - lastSentAt) / 1000));
}

/** Forgot-password submit label: countdown while throttled, resend after the first send. */
export function forgotSubmitLabel(resendIn: number, sent: boolean): string {
  if (resendIn > 0) {
    return `Resend in ${resendIn} ${resendIn === 1 ? "second" : "seconds"}`;
  }
  return sent ? "Resend link" : "Send reset link";
}

/** True when the failure means the address has no account (never surfaced verbatim). */
export function isUnknownAccountFailure(error: unknown): boolean {
  if (!isApiError(error)) return false;
  return error.code === UNKNOWN_ACCOUNT_CODE || error.message === UNKNOWN_ACCOUNT_MESSAGE;
}

/**
 * Friendliest usable detail for a failed auth POST: the card matrix
 * message when the server code is known, else the raw server detail, else
 * null (network failures and non-API errors carry nothing to show).
 */
function failureDetail(error: unknown): string | null {
  if (!isApiError(error)) return null;
  const mapped = describeAuthError(error.code, "sign-in", "");
  if (mapped) return mapped.message;
  if (error.status > 0 && error.message.length > 0) return error.message;
  return null;
}

/**
 * Forgot-password failure toast body: the server detail when one exists,
 * a generic fallback otherwise — and never any unknown-account wording,
 * even for addresses with no account (AUTH-012).
 */
export function describeForgotPasswordFailure(error: unknown): string {
  if (isUnknownAccountFailure(error)) return FORGOT_FALLBACK_MESSAGE;
  if (isApiError(error)) {
    const override = FORGOT_MESSAGE_OVERRIDES[error.code];
    if (override !== undefined) return override;
  }
  return failureDetail(error) ?? FORGOT_FALLBACK_MESSAGE;
}

/** Set-password failure toast body: server detail when one exists, generic otherwise. */
export function describeSetPasswordFailure(error: unknown): string {
  return failureDetail(error) ?? SET_FALLBACK_MESSAGE;
}

export interface ResetFailure {
  message: string;
  /** Dead links replace the form with the banner; other failures keep it for retry. */
  tokenDead: boolean;
}

/** Banner for a reset POST the server answered with a failure redirect. */
export function describeResetFailure(result: { code: string; message: string }): ResetFailure {
  if (result.code === INVALID_TOKEN_CODE) return { message: INVALID_TOKEN_BANNER, tokenDead: true };
  if (result.code === EXPIRED_TOKEN_CODE) return { message: EXPIRED_TOKEN_BANNER, tokenDead: true };
  const mapped = describeAuthError(result.code, "sign-in", "");
  return { message: mapped?.message ?? result.message, tokenDead: false };
}

/**
 * Banner for a reset POST that threw instead of answering. A 5xx from this
 * endpoint is the malformed-uid crash (NEWFRONT-130): per AUTH-012-014 the
 * intended behavior is the invalid-token banner, so the form is replaced
 * just like a dead link. Anything else keeps the form for retry.
 */
export function describeResetException(error: unknown): ResetFailure {
  if (isApiError(error) && error.status >= 500) {
    return { message: INVALID_TOKEN_BANNER, tokenDead: true };
  }
  return { message: failureDetail(error) ?? RESET_FALLBACK_MESSAGE, tokenDead: false };
}

export type PasswordStrength = "empty" | "weak" | "strong";

/**
 * Client-side secret meter (AUTH-010). The server (zxcvbn) stays
 * authoritative; this only gates the submit so weak secrets never post.
 * Strong means long enough with at least three of the four classes.
 */
export function secretStrength(password: string): PasswordStrength {
  if (password.length === 0) return "empty";
  if (password.length < MIN_SECRET_LENGTH) return "weak";
  let classes = 0;
  if (/[a-z]/.test(password)) classes += 1;
  if (/[A-Z]/.test(password)) classes += 1;
  if (/[0-9]/.test(password)) classes += 1;
  if (/[^a-zA-Z0-9]/.test(password)) classes += 1;
  return classes >= 3 ? "strong" : "weak";
}

/** Indicator text while the secret field holds a weak value; null otherwise. */
export function weakSecretHint(password: string): string | null {
  if (password.length === 0) return null;
  if (password.length < MIN_SECRET_LENGTH) return "Use at least 8 characters.";
  return secretStrength(password) === "weak" ? "Weak password — add mixed case, numbers, or symbols." : null;
}

/**
 * Mismatch message timing (AUTH-010): once the confirmation loses focus
 * or grows at least as long as the secret.
 */
export function passwordMismatchError(password: string, confirm: string, confirmBlurred: boolean): string | null {
  if (confirm.length === 0 || confirm === password) return null;
  if (!confirmBlurred && confirm.length < password.length) return null;
  return "The two passwords do not match.";
}

/** Submit gate: the secret is strong and both fields match. */
export function canSubmitNewPassword(password: string, confirm: string): boolean {
  return secretStrength(password) === "strong" && password === confirm;
}
