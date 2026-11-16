// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Two-step email change rules (parity/specs/profile.md §6, PROF-013). The
// first step checks the address shape, refuses a taken address, then asks
// the server to mail a one-time code; the second locks the address and
// verifies the code. Wrong codes and server refusals report inline on the
// visible step.

export type EmailChangeStep = "email" | "code";

/** A practical address shape check for the first step. */
const EMAIL_SHAPE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export const EMAIL_MESSAGES = {
  required: "Email is required",
  invalid: "Email is invalid",
  taken: "Email already exists. Please use a different one.",
  unchanged: "Use an address different from your current one.",
  codeRequired: "Unique code is required",
  codeInvalid: "Invalid verification code",
  sent: "Verification code sent to your new email.",
  failed: "Email validation failed. Please try again.",
} as const;

/** The first step's inline error for a typed address, or null when it may proceed. */
export function emailShapeError(email: string, currentEmail: string): string | null {
  const trimmed = email.trim();
  if (trimmed === "") return EMAIL_MESSAGES.required;
  if (!EMAIL_SHAPE.test(trimmed)) return EMAIL_MESSAGES.invalid;
  if (trimmed.toLowerCase() === currentEmail.trim().toLowerCase()) return EMAIL_MESSAGES.unchanged;
  return null;
}

/** The second step's inline error for a typed code, or null when it may submit. */
export function emailCodeError(code: string): string | null {
  return code.trim() === "" ? EMAIL_MESSAGES.codeRequired : null;
}

/**
 * Map a refusal from the first step's taken check or code send to the
 * inline message. The server answers `{error}` for the email write and
 * numeric auth error codes for the entry calls.
 */
export function describeEmailSendFailure(error: unknown): string {
  const serverMessage = serverErrorText(error);
  if (serverMessage === null) return EMAIL_MESSAGES.failed;
  if (/already exists/i.test(serverMessage)) return EMAIL_MESSAGES.taken;
  if (/invalid email|not look valid/i.test(serverMessage)) return EMAIL_MESSAGES.invalid;
  if (/different from current/i.test(serverMessage)) return EMAIL_MESSAGES.unchanged;
  return serverMessage;
}

/** Body shape carried by an ApiError for a refusal, if any. */
function serverErrorText(error: unknown): string | null {
  const record = (error ?? {}) as { message?: unknown; body?: unknown };
  const body = record.body as { error?: unknown } | null | undefined;
  if (typeof body?.error === "string" && body.error.length > 0) return body.error;
  if (typeof record.message === "string" && record.message.length > 0) return record.message;
  return null;
}

/**
 * Step outcome from a taken-address check: a taken address reports inline
 * and never asks the server to mail a code; anything else advances only
 * once the code send succeeds.
 */
export type EmailStepOutcome = { kind: "taken" | "failed"; message: string } | { kind: "sent" };

export function emailStepOutcome(args: { existing: boolean; sendFailure?: unknown }): EmailStepOutcome {
  if (args.existing) return { kind: "taken", message: EMAIL_MESSAGES.taken };
  if (args.sendFailure !== undefined) {
    return { kind: "failed", message: describeEmailSendFailure(args.sendFailure) };
  }
  return { kind: "sent" };
}

/** Inline message for a failed verification; a bare server refusal keeps its text. */
export function emailVerifyMessage(error: unknown): string {
  return serverErrorText(error) ?? EMAIL_MESSAGES.codeInvalid;
}
