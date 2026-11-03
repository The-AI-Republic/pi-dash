// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { selectMemberOptions } from "./members.js";

const ME = "11111111-1111-4111-8111-111111111111";
const ANA = "22222222-2222-4222-8222-222222222222";
const ZOE = "33333333-3333-4333-8333-333333333333";
const GUEST = "44444444-4444-4444-8444-444444444444";
const PROJECT = "55555555-5555-4555-8555-555555555555";

function directoryRow(userId: string, displayName: string, role = 15, isActive: boolean | undefined = true) {
  return {
    id: `${userId}-membership`,
    member: {
      id: userId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
    },
    role,
    ...(isActive === undefined ? {} : { is_active: isActive }),
  };
}

function projectRow(userId: string, role = 15) {
  return {
    id: `${userId}-project`,
    role,
    member: userId,
    project: PROJECT,
    original_role: role,
    created_at: "2026-01-01T00:00:00.000Z",
  };
}

describe("selectMemberOptions", () => {
  it("joins project members to directory names with You first", () => {
    const options = selectMemberOptions({
      projectMembers: [projectRow(ANA), projectRow(ME)],
      directory: [directoryRow(ME, "Parity User"), directoryRow(ANA, "Ana")],
      meId: ME,
      selectedIds: [],
    });
    expect(options.map((option) => option.label)).toEqual(["You", "Ana"]);
    expect(options[0]).toMatchObject({ userId: ME, searchText: "Parity User", selected: false, suspended: false });
  });

  it("sorts selected members after the current user", () => {
    const options = selectMemberOptions({
      projectMembers: [projectRow(ANA), projectRow(ZOE), projectRow(ME)],
      directory: [directoryRow(ME, "Parity User"), directoryRow(ANA, "Ana"), directoryRow(ZOE, "Zoe")],
      meId: ME,
      selectedIds: [ZOE],
    });
    expect(options.map((option) => option.userId)).toEqual([ME, ZOE, ANA]);
    expect(options[1]).toMatchObject({ selected: true });
  });

  it("sorts without a signed-in member", () => {
    const options = selectMemberOptions({
      projectMembers: [projectRow(ANA), projectRow(ZOE)],
      directory: [directoryRow(ANA, "Ana"), directoryRow(ZOE, "Zoe")],
      meId: null,
      selectedIds: [ZOE],
    });
    expect(options.map((option) => option.userId)).toEqual([ZOE, ANA]);
  });

  it("excludes guests in both scopes", () => {
    const project = selectMemberOptions({
      projectMembers: [projectRow(ANA), projectRow(GUEST, 5)],
      directory: [directoryRow(ANA, "Ana"), directoryRow(GUEST, "Parity Guest", 5)],
      meId: null,
      selectedIds: [],
    });
    expect(project.map((option) => option.userId)).toEqual([ANA]);
    const workspace = selectMemberOptions({
      directory: [directoryRow(ANA, "Ana"), directoryRow(GUEST, "Parity Guest", 5)],
      meId: null,
      selectedIds: [],
    });
    expect(workspace.map((option) => option.userId)).toEqual([ANA]);
  });

  it("marks deactivated members suspended", () => {
    const options = selectMemberOptions({
      projectMembers: [projectRow(ANA), projectRow(ZOE)],
      directory: [directoryRow(ANA, "Ana"), directoryRow(ZOE, "Zoe", 15, false)],
      meId: null,
      selectedIds: [],
    });
    expect(options.find((option) => option.userId === ZOE)).toMatchObject({ suspended: true });
    expect(options.find((option) => option.userId === ANA)).toMatchObject({ suspended: false });
  });

  it("treats a missing suspended flag as active", () => {
    const options = selectMemberOptions({
      directory: [directoryRow(ANA, "Ana", 15, undefined)],
      meId: null,
      selectedIds: [],
    });
    expect(options).toMatchObject([{ suspended: false }]);
  });

  it("names project members missing from the directory", () => {
    const options = selectMemberOptions({
      projectMembers: [projectRow(ANA)],
      directory: [],
      meId: null,
      selectedIds: [],
    });
    expect(options).toMatchObject([{ userId: ANA, label: "Unknown member", suspended: false }]);
  });

  it("offers the directory alone in workspace scope", () => {
    const options = selectMemberOptions({
      directory: [directoryRow(ANA, "Ana"), directoryRow(ME, "Parity User")],
      meId: ME,
      selectedIds: [ANA],
    });
    expect(options.map((option) => option.label)).toEqual(["You", "Ana"]);
  });
});
