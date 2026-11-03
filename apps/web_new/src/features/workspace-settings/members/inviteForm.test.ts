// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk-invite form rules (WSET-009, spec §10): rows append and drop,
// dropping the last row leaves one fresh row, blank and malformed
// addresses carry their own inline message, and a valid form maps to the
// request body with trimmed addresses.
import { describe, expect, it } from "vitest";

import {
  appendInviteRow,
  createInviteRow,
  INVITE_EMAIL_INVALID,
  INVITE_EMAIL_REQUIRED,
  invitePayload,
  removeInviteRow,
  setInviteRowEmail,
  setInviteRowRole,
  validateInviteEmail,
  validateInviteRows,
} from "./inviteForm.js";

describe("invite rows", () => {
  it("creates a blank Member row and keys every row uniquely", () => {
    const first = createInviteRow();
    const second = createInviteRow();
    expect(first).toMatchObject({ email: "", role: 15 });
    expect(first.key).not.toBe(second.key);
  });

  it("appends with the requested role", () => {
    const rows = appendInviteRow([createInviteRow()], 5);
    expect(rows).toHaveLength(2);
    expect(rows[1]).toMatchObject({ email: "", role: 5 });
  });

  it("drops one row and keeps the rest untouched", () => {
    const first = createInviteRow();
    const second = createInviteRow();
    const kept = removeInviteRow([first, second], first.key);
    expect(kept).toEqual([second]);
  });

  it("re-appends a single row when the last one is dropped", () => {
    const row = createInviteRow(5);
    const replaced = removeInviteRow([row], row.key);
    expect(replaced).toHaveLength(1);
    expect(replaced[0]).toMatchObject({ email: "", role: 5 });
    expect(replaced[0]!.key).not.toBe(row.key);
  });

  it("falls back to the default role when an empty list has no role to keep", () => {
    const replaced = removeInviteRow([], "missing");
    expect(replaced).toEqual([expect.objectContaining({ email: "", role: 15 })]);
  });

  it("rewrites one row's address or role and leaves its siblings alone", () => {
    const first = createInviteRow();
    const second = createInviteRow();
    const emails = setInviteRowEmail([first, second], second.key, "ada@example.com");
    expect(emails[0]).toEqual(first);
    expect(emails[1]!.email).toBe("ada@example.com");
    const roles = setInviteRowRole(emails, second.key, 20);
    expect(roles[1]).toMatchObject({ email: "ada@example.com", role: 20 });
    expect(roles[0]).toEqual(first);
  });
});

describe("invite validation", () => {
  it.each([
    ["", INVITE_EMAIL_REQUIRED],
    ["   ", INVITE_EMAIL_REQUIRED],
    ["not-an-email", INVITE_EMAIL_INVALID],
    ["ada@", INVITE_EMAIL_INVALID],
    ["@example.com", INVITE_EMAIL_INVALID],
    ["ada@example", INVITE_EMAIL_INVALID],
    ["ada@@example.com", INVITE_EMAIL_INVALID],
    ["ada @example.com", INVITE_EMAIL_INVALID],
  ])("rejects %j with %j", (email, expected) => {
    expect(validateInviteEmail(email)).toBe(expected);
  });

  it.each(["ada@example.com", "ada.lovelace@example.co.uk", "  ada@example.com  "])("accepts %j", (email) => {
    expect(validateInviteEmail(email)).toBeNull();
  });

  it("keys errors by row and leaves valid rows out of the map", () => {
    const valid = { ...createInviteRow(), email: "ada@example.com" };
    const blank = createInviteRow();
    const broken = { ...createInviteRow(), email: "nope" };
    expect(validateInviteRows([valid, blank, broken])).toEqual({
      [blank.key]: INVITE_EMAIL_REQUIRED,
      [broken.key]: INVITE_EMAIL_INVALID,
    });
  });

  it("maps a valid form to trimmed email-plus-role pairs", () => {
    const rows = [
      { ...createInviteRow(), email: " ada@example.com ", role: 5 },
      { ...createInviteRow(), email: "grace@example.com", role: 20 },
    ];
    expect(invitePayload(rows)).toEqual([
      { email: "ada@example.com", role: 5 },
      { email: "grace@example.com", role: 20 },
    ]);
  });
});
