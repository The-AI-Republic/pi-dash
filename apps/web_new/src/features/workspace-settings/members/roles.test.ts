// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role choices (WSET-009/010/012, spec §10–§12): an actor offers only the
// roles at or below their own, a missing role offers nothing, and the
// invite default settles on Member whenever it is assignable.
import { describe, expect, it } from "vitest";

import {
  ASSIGNABLE_ROLES,
  assignableRoleLabels,
  assignableRoles,
  DEFAULT_INVITE_ROLE,
  initialInviteRole,
  isAboveActor,
  roleLabelFor,
  roleValueForLabel,
} from "./roles.js";

describe("assignable roles", () => {
  it("lists the least privileged first with the backend numbers", () => {
    expect(ASSIGNABLE_ROLES.map((role) => [role.label, role.value])).toEqual([
      ["Guest", 5],
      ["Member", 15],
      ["Admin", 20],
    ]);
  });

  it.each([
    [20, ["Guest", "Member", "Admin"]],
    [15, ["Guest", "Member"]],
    [5, ["Guest"]],
    [0, []],
    [null, []],
  ])("an actor at %s may assign %j", (actorRole, expected) => {
    expect(assignableRoleLabels(actorRole)).toEqual(expected);
    expect(assignableRoles(actorRole).map((role) => role.value)).toEqual(
      ASSIGNABLE_ROLES.filter((role) => role.value <= (actorRole ?? 0)).map((role) => role.value)
    );
  });

  it("maps a label back to its number only while the actor may assign it", () => {
    expect(roleValueForLabel(20, "Admin")).toBe(20);
    expect(roleValueForLabel(15, "Admin")).toBeNull();
    expect(roleValueForLabel(null, "Guest")).toBeNull();
    expect(roleValueForLabel(20, "Owner")).toBeNull();
  });

  it("flags every role a missing actor cannot hand out", () => {
    expect(isAboveActor(null, 5)).toBe(true);
    expect(isAboveActor(15, 20)).toBe(true);
    expect(isAboveActor(20, 20)).toBe(false);
    expect(isAboveActor(20, 5)).toBe(false);
  });

  it("defaults a new invite row to Member, or the highest assignable role", () => {
    expect(DEFAULT_INVITE_ROLE).toBe(15);
    expect(initialInviteRole(20)).toBe(15);
    expect(initialInviteRole(15)).toBe(15);
    expect(initialInviteRole(5)).toBe(5);
    expect(initialInviteRole(0)).toBe(DEFAULT_INVITE_ROLE);
    expect(initialInviteRole(null)).toBe(DEFAULT_INVITE_ROLE);
  });

  it("labels known roles and clamps unknown numbers like the roster", () => {
    expect(roleLabelFor(5)).toBe("Guest");
    expect(roleLabelFor(15)).toBe("Member");
    expect(roleLabelFor(20)).toBe("Admin");
    expect(roleLabelFor(25)).toBe("Admin");
    expect(roleLabelFor(18)).toBe("Member");
    expect(roleLabelFor(0)).toBe("Guest");
  });
});
