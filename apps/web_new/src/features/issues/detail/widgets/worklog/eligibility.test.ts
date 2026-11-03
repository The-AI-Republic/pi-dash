// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { canLogWorklog, canManageWorklogEntry, type WorklogStanding } from "./eligibility.js";

const VIEWER = "123e4567-e89b-12d3-a456-426614174000";
const OTHER = "223e4567-e89b-12d3-a456-426614174001";

const BASE: WorklogStanding = {
  memberRole: 15,
  viewerId: VIEWER,
  assigneeIds: [VIEWER],
  isIntake: false,
  archived: false,
};

describe("canLogWorklog", () => {
  it("admits an assigned member", () => {
    expect(canLogWorklog(BASE)).toBe(true);
  });

  it("admits an admin whether assigned or not", () => {
    expect(canLogWorklog({ ...BASE, memberRole: 20, assigneeIds: [] })).toBe(true);
    expect(canLogWorklog({ ...BASE, memberRole: 20 })).toBe(true);
  });

  it("refuses an unassigned member", () => {
    expect(canLogWorklog({ ...BASE, assigneeIds: [OTHER] })).toBe(false);
    expect(canLogWorklog({ ...BASE, assigneeIds: [] })).toBe(false);
  });

  it("refuses guests, even assigned ones, and unknown roles", () => {
    expect(canLogWorklog({ ...BASE, memberRole: 5 })).toBe(false);
    expect(canLogWorklog({ ...BASE, memberRole: null })).toBe(false);
    expect(canLogWorklog({ ...BASE, memberRole: 99 })).toBe(false);
  });

  it("refuses intake and archived items", () => {
    expect(canLogWorklog({ ...BASE, isIntake: true })).toBe(false);
    expect(canLogWorklog({ ...BASE, archived: true })).toBe(false);
  });

  it("refuses an unknown viewer", () => {
    expect(canLogWorklog({ ...BASE, viewerId: null })).toBe(false);
  });
});

describe("canManageWorklogEntry", () => {
  it("admits the author on a live item", () => {
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 15, entry: { actor: VIEWER }, archived: false })).toBe(
      true
    );
  });

  it("admits an admin on another member's entry", () => {
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 20, entry: { actor: OTHER }, archived: false })).toBe(
      true
    );
  });

  it("refuses a non-admin on another member's entry", () => {
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 15, entry: { actor: OTHER }, archived: false })).toBe(
      false
    );
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 5, entry: { actor: OTHER }, archived: false })).toBe(
      false
    );
    expect(
      canManageWorklogEntry({ viewerId: VIEWER, memberRole: null, entry: { actor: OTHER }, archived: false })
    ).toBe(false);
  });

  it("refuses an unknown viewer without an admin role", () => {
    expect(canManageWorklogEntry({ viewerId: null, memberRole: 15, entry: { actor: OTHER }, archived: false })).toBe(
      false
    );
    expect(canManageWorklogEntry({ viewerId: null, memberRole: null, entry: { actor: OTHER }, archived: false })).toBe(
      false
    );
  });

  it("refuses everyone, including admins, on archived items", () => {
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 20, entry: { actor: OTHER }, archived: true })).toBe(
      false
    );
    expect(canManageWorklogEntry({ viewerId: VIEWER, memberRole: 15, entry: { actor: VIEWER }, archived: true })).toBe(
      false
    );
  });
});
