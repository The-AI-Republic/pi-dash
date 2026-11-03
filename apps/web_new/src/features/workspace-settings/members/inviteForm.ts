// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk-invite form rules (parity/specs/workspace-settings.md §10,
// WSET-009). The dialog holds a list of email-plus-role rows. Each row
// validates on submit: blank is required, anything that is not an address
// is invalid; valid rows become the `{ emails: [{ email, role }] }` body.
// An empty list gets one fresh row back, never a dialog with nothing to
// fill in.

import type { WorkspaceInvitee } from "@pidash/api-client";

import { DEFAULT_INVITE_ROLE } from "./roles.js";

export interface InviteRow {
  /** Local identity so React and the remove control never key on index. */
  key: string;
  email: string;
  role: number;
}

/** Inline messages (§10; the invalid-address wording is oracle-pinned). */
export const INVITE_EMAIL_REQUIRED = "Email is required";
export const INVITE_EMAIL_INVALID = "Email is invalid";

/**
 * Address shape the dialog accepts: one local part, one `@`, a dotted
 * domain. Deliberately permissive — the server is the final authority —
 * but strict enough that "not-an-email" never leaves the dialog.
 */
const EMAIL_SHAPE = /^[^\s@]+@[^\s@.]+(\.[^\s@.]+)+$/;

let rowCounter = 0;

/** A new row; roles default to Member. */
export function createInviteRow(role: number = DEFAULT_INVITE_ROLE): InviteRow {
  rowCounter += 1;
  return { key: `invite-row-${rowCounter}`, email: "", role };
}

/** Append a defaulted row to the list. */
export function appendInviteRow(rows: readonly InviteRow[], role: number = DEFAULT_INVITE_ROLE): InviteRow[] {
  return [...rows, createInviteRow(role)];
}

/**
 * Drop one row. A list that would empty itself is replaced by one fresh
 * row instead, so the dialog always has an address field (§10).
 */
export function removeInviteRow(rows: readonly InviteRow[], key: string): InviteRow[] {
  const kept = rows.filter((row) => row.key !== key);
  return kept.length === 0 ? [createInviteRow(rows[0]?.role ?? DEFAULT_INVITE_ROLE)] : kept;
}

/** Rewrite one row's address. */
export function setInviteRowEmail(rows: readonly InviteRow[], key: string, email: string): InviteRow[] {
  return rows.map((row) => (row.key === key ? { ...row, email } : row));
}

/** Rewrite one row's role. */
export function setInviteRowRole(rows: readonly InviteRow[], key: string, role: number): InviteRow[] {
  return rows.map((row) => (row.key === key ? { ...row, role } : row));
}

/** Inline message for one address, or null when it can be sent. */
export function validateInviteEmail(email: string): string | null {
  const trimmed = email.trim();
  if (trimmed === "") return INVITE_EMAIL_REQUIRED;
  if (!EMAIL_SHAPE.test(trimmed)) return INVITE_EMAIL_INVALID;
  return null;
}

/** Inline messages keyed by row key; every key present is blocked. */
export function validateInviteRows(rows: readonly InviteRow[]): Record<string, string> {
  const errors: Record<string, string> = {};
  for (const row of rows) {
    const message = validateInviteEmail(row.email);
    if (message !== null) errors[row.key] = message;
  }
  return errors;
}

/** The request body for a valid form: trimmed addresses, one entry per row. */
export function invitePayload(rows: readonly InviteRow[]): WorkspaceInvitee[] {
  return rows.map((row) => ({ email: row.email.trim(), role: row.role }));
}
