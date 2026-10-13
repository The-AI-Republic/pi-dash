// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI device-approval rules (AUTH-021, from parity/specs/auth.md): the
// short code auto-formats as typed (XXXX-YYYY), submit needs eight code
// characters, and failures surface the server detail with a generic retry
// line as the fallback.

/** Code characters per approval code, excluding the dash. */
export const DEVICE_CODE_LENGTH = 8;

/** Longest field value: eight code characters plus the dash. */
export const DEVICE_CODE_FIELD_LENGTH = 9;

/** Retry line when the server returns no error detail. */
export const DEVICE_APPROVE_FAILED_MESSAGE = "That code could not be approved. Confirm it and try again.";

/** Strip everything but alphanumerics and uppercase the rest. */
export function normalizeDeviceCode(raw: string): string {
  return raw.replace(/[^a-z0-9]/gi, "").toUpperCase();
}

/** Dashed groups as typed (XXXX-YYYY), capped at eight code characters. */
export function formatDeviceCode(raw: string): string {
  const clean = normalizeDeviceCode(raw).slice(0, DEVICE_CODE_LENGTH);
  if (clean.length <= 4) return clean;
  return `${clean.slice(0, 4)}-${clean.slice(4)}`;
}

/** True once eight code characters are present, dash or not. */
export function isCompleteDeviceCode(value: string): boolean {
  return normalizeDeviceCode(value).length === DEVICE_CODE_LENGTH;
}

/** The server detail for a failed approval, or the generic retry line. */
export function describeDeviceApprovalFailure(error: unknown): string {
  const detail = error instanceof Error ? error.message : "";
  return detail === "" ? DEVICE_APPROVE_FAILED_MESSAGE : detail;
}
