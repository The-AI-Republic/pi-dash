// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Write-side rules (parity/specs/project-settings-members.md §"Add members",
// §"Inline role changes", §"Remove and leave-self", §"Project defaults";
// PSET-022–026): offered colleagues, per-row workspace caps, role-edit
// eligibility, row menu verbs and defaults choices.
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { MemberRosterRow } from "./roster.js";
import {
  appendInviteDraftRow,
  canEditMemberRole,
  createInviteDraftRow,
  DEFAULTS_NONE_LABEL,
  defaultMemberOptions,
  defaultSlotLabel,
  INVITE_MEMBER_REQUIRED,
  inviteCandidates,
  inviteDraftPayload,
  memberRowAction,
  pickInviteDraftMember,
  removeInviteDraftRow,
  roleForWorkspaceRole,
  roleNumber,
  rolesForTarget,
  rolesForWorkspaceRole,
  roleWord,
  setInviteDraftRole,
  validateInviteDraftRows,
  type InviteCandidate,
} from "./writes.js";

const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";
const LINUS = "123e4567-e89b-12d3-a456-42661417400c";
const BOT = "123e4567-e89b-12d3-a456-42661417400d";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function directoryEntry(id: string, overrides: Partial<WorkspaceMemberList[number]> = {}): WorkspaceMemberList[number] {
  return {
    id: `${id}-membership`,
    role: 15,
    member: {
      id,
      first_name: "First",
      last_name: id.slice(-1),
      avatar: "",
      avatar_url: null,
      display_name: `display-${id.slice(-1)}`,
      is_bot: false,
    },
    ...overrides,
  };
}

const DIRECTORY: WorkspaceMemberList = [
  directoryEntry(ADA, {
    role: 20,
    member: {
      id: ADA,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: "/media/ada.png",
      display_name: "ada",
      is_bot: false,
    },
  }),
  directoryEntry(GRACE, {
    member: {
      id: GRACE,
      first_name: "Grace",
      last_name: "Hopper",
      avatar: "",
      avatar_url: null,
      display_name: "",
      is_bot: false,
    },
  }),
  directoryEntry(LINUS, { role: 5 }),
  directoryEntry(BOT, { member: { ...directoryEntry(BOT).member, is_bot: true } }),
];

const MEMBERSHIPS: ProjectMemberList = [
  { id: "m-ada", role: 20, member: ADA, project: PROJECT, original_role: 20, created_at: "2025-01-01T00:00:00Z" },
];

function candidate(overrides: Partial<InviteCandidate> = {}): InviteCandidate {
  return {
    userId: GRACE,
    workspaceRole: 15,
    fullName: "Grace Hopper",
    displayName: "grace",
    avatarUrl: null,
    ...overrides,
  };
}

describe("role words", () => {
  it("maps role numbers to their words and back", () => {
    expect(roleWord(20)).toBe("Admin");
    expect(roleWord(15)).toBe("Member");
    expect(roleWord(5)).toBe("Guest");
    expect(roleWord(7)).toBe("Guest");
    expect(roleNumber("Admin")).toBe(20);
    expect(roleNumber("Guest")).toBe(5);
    expect(roleNumber("Owner")).toBeNull();
  });

  it("enters each workspace role at its own level", () => {
    expect(roleForWorkspaceRole(20)).toBe(20);
    expect(roleForWorkspaceRole(15)).toBe(15);
    expect(roleForWorkspaceRole(5)).toBe(5);
  });
});

describe("invite role caps (PSET-022)", () => {
  it("admits every role for a workspace member, listed lowest first", () => {
    expect(rolesForWorkspaceRole(15).map((option) => option.label)).toEqual(["Guest", "Member", "Admin"]);
  });

  it("caps a workspace guest at Guest and a workspace admin at Admin", () => {
    expect(rolesForWorkspaceRole(5).map((option) => option.label)).toEqual(["Guest"]);
    expect(rolesForWorkspaceRole(20).map((option) => option.label)).toEqual(["Admin"]);
  });

  it("caps a workspace guest at Guest while a demoted project guest still offers every role", () => {
    // The cap follows the workspace role only: restoring a demotion
    // needs Member and Admin offered to a project guest (PSET-023).
    expect(rolesForTarget(5).map((option) => option.label)).toEqual(["Guest"]);
    expect(rolesForTarget(15).map((option) => option.label)).toEqual(["Guest", "Member", "Admin"]);
    expect(rolesForTarget(20).map((option) => option.label)).toEqual(["Guest", "Member", "Admin"]);
  });
});

describe("invite candidates (PSET-022)", () => {
  it("offers workspace colleagues without a membership, bots aside", () => {
    const candidates = inviteCandidates(DIRECTORY, MEMBERSHIPS);
    expect(candidates.map((entry) => entry.userId)).toEqual([GRACE, LINUS]);
  });

  it("keeps the workspace role, falls back to the joined name and drops blanks", () => {
    const candidates = inviteCandidates(DIRECTORY, MEMBERSHIPS);
    // Grace has no display name: the joined name stands in for both fields.
    expect(candidates[0]).toEqual({
      userId: GRACE,
      workspaceRole: 15,
      fullName: "Grace Hopper",
      displayName: "Grace Hopper",
      avatarUrl: null,
    });
    // A candidate with no names at all still keeps a finite display name.
    const nameless = inviteCandidates(
      [
        {
          ...directoryEntry(GRACE),
          member: { ...directoryEntry(GRACE).member, first_name: "", last_name: "", display_name: "" },
        },
      ],
      []
    );
    expect(nameless[0]?.fullName).toBe("");
  });
});

describe("invite draft rows (PSET-022)", () => {
  it("starts empty, appends rows and refuses to empty itself", () => {
    const first = createInviteDraftRow();
    expect(first.memberId).toBeNull();
    expect(first.role).toBe(5);
    const two = appendInviteDraftRow([first]);
    expect(two).toHaveLength(2);
    expect(removeInviteDraftRow(two, two[1]!.key)).toHaveLength(1);
    const replaced = removeInviteDraftRow([first], first.key);
    expect(replaced).toHaveLength(1);
    expect(replaced[0]?.key).not.toBe(first.key);
  });

  it("defaults the row role to the colleague's workspace role on pick", () => {
    const first = createInviteDraftRow();
    const picked = pickInviteDraftMember([first], first.key, candidate({ workspaceRole: 20 }));
    expect(picked[0]).toMatchObject({ memberId: GRACE, role: 20 });
    // Picking another row leaves this one untouched.
    const second = createInviteDraftRow();
    const rows = [picked[0]!, second];
    expect(pickInviteDraftMember(rows, second.key, candidate({ workspaceRole: 5 }))[0]).toBe(picked[0]);
  });

  it("changes one row's role only", () => {
    const first = createInviteDraftRow();
    const second = createInviteDraftRow();
    const rows = setInviteDraftRole([first, second], second.key, 20);
    expect(rows[0]?.role).toBe(5);
    expect(rows[1]?.role).toBe(20);
  });

  it("blocks rows without a colleague and reports them by key", () => {
    const first = createInviteDraftRow();
    const second = createInviteDraftRow();
    const rows = pickInviteDraftMember([first, second], second.key, candidate());
    expect(validateInviteDraftRows(rows)).toEqual({ [first.key]: INVITE_MEMBER_REQUIRED });
    expect(validateInviteDraftRows(rows.slice(1))).toEqual({});
  });

  it("builds the bulk payload in row order", () => {
    const first = createInviteDraftRow();
    const rows = setInviteDraftRole(
      pickInviteDraftMember([first], first.key, candidate({ workspaceRole: 15 })),
      first.key,
      20
    );
    expect(inviteDraftPayload(rows)).toEqual([{ member_id: GRACE, role: 20 }]);
    expect(inviteDraftPayload([createInviteDraftRow()])).toEqual([{ member_id: "", role: 5 }]);
  });
});

describe("role-edit eligibility (PSET-023, pin NEWFRONT-401)", () => {
  const others = { userId: GRACE, workspaceRole: 15 };

  it("lets a workspace admin edit their own row and non-admins' rows", () => {
    const self = { userId: ADA, workspaceRole: 20, projectRole: 20 };
    expect(canEditMemberRole(self, { userId: ADA, workspaceRole: 20 })).toBe(true);
    expect(canEditMemberRole(self, others)).toBe(true);
    expect(canEditMemberRole(self, { userId: LINUS, workspaceRole: 20 })).toBe(false);
  });

  it("lets a project admin edit others but not themselves or a workspace admin", () => {
    const admin = { userId: GRACE, workspaceRole: 15, projectRole: 20 };
    expect(canEditMemberRole(admin, { userId: ADA, workspaceRole: 20 })).toBe(false);
    expect(canEditMemberRole(admin, { userId: GRACE, workspaceRole: 15 })).toBe(false);
    expect(canEditMemberRole(admin, { userId: LINUS, workspaceRole: 5 })).toBe(true);
  });

  it("leaves plain members and guests read-only", () => {
    expect(
      canEditMemberRole({ userId: GRACE, workspaceRole: 15, projectRole: 15 }, { userId: LINUS, workspaceRole: 5 })
    ).toBe(false);
    expect(
      canEditMemberRole({ userId: GRACE, workspaceRole: 5, projectRole: 5 }, { userId: LINUS, workspaceRole: 5 })
    ).toBe(false);
    expect(canEditMemberRole({ userId: null, workspaceRole: null, projectRole: null }, others)).toBe(false);
  });
});

describe("row menu verbs (PSET-024/025)", () => {
  it("offers leave on one's own row regardless of standing", () => {
    const editor = { userId: ADA, workspaceRole: 20, projectRole: 20 };
    expect(memberRowAction(editor, { userId: ADA, workspaceRole: 20 })).toBe("leave");
  });

  it("offers remove to project and workspace admins on other rows", () => {
    expect(
      memberRowAction({ userId: GRACE, workspaceRole: 15, projectRole: 20 }, { userId: LINUS, workspaceRole: 5 })
    ).toBe("remove");
    expect(
      memberRowAction({ userId: GRACE, workspaceRole: 20, projectRole: 15 }, { userId: LINUS, workspaceRole: 5 })
    ).toBe("remove");
  });

  it("offers nothing to everyone else", () => {
    expect(
      memberRowAction({ userId: GRACE, workspaceRole: 15, projectRole: 15 }, { userId: LINUS, workspaceRole: 5 })
    ).toBe("none");
    expect(
      memberRowAction({ userId: null, workspaceRole: null, projectRole: null }, { userId: LINUS, workspaceRole: 5 })
    ).toBe("none");
  });
});

function rosterRow(overrides: Partial<MemberRosterRow>): MemberRosterRow {
  return {
    membershipId: "m",
    userId: ADA,
    firstName: "Ada",
    lastName: "Lovelace",
    rawDisplayName: "ada",
    fullName: "Ada Lovelace",
    displayName: "ada",
    email: "",
    avatarUrl: null,
    role: 20,
    roleLabel: "Admin",
    joinedAt: "2025-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("defaults choices (PSET-026)", () => {
  it("lists non-guest members and skips guests", () => {
    const options = defaultMemberOptions([
      rosterRow({ userId: ADA, displayName: "ada", role: 20 }),
      rosterRow({ userId: GRACE, displayName: "grace", role: 15 }),
      rosterRow({ userId: LINUS, displayName: "linus", role: 5 }),
    ]);
    expect(options).toEqual([
      { userId: ADA, label: "ada" },
      { userId: GRACE, label: "grace" },
    ]);
  });

  it("labels the empty slot None and unknown ids also read None", () => {
    const options = [{ userId: ADA, label: "ada" }];
    expect(defaultSlotLabel(options, null)).toBe(DEFAULTS_NONE_LABEL);
    expect(defaultSlotLabel(options, ADA)).toBe("ada");
    expect(defaultSlotLabel(options, GRACE)).toBe(DEFAULTS_NONE_LABEL);
  });
});
