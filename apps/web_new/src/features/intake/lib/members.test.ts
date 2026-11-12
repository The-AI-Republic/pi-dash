// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { selectIntakeMemberOptions } from "./members.js";

function projectMember(userId: string, role = 15) {
  return {
    id: `membership-${userId}`,
    role,
    member: userId,
    project: "223e4567-e89b-12d3-a456-426614174001",
    original_role: role,
    created_at: "2026-10-01T00:00:00Z",
  };
}

function directoryEntry(userId: string, displayName: string, active = true, role = 15) {
  return {
    id: `workspace-${userId}`,
    member: {
      id: userId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
      email: `${userId}@example.com`,
    },
    role,
    is_active: active,
  };
}

const PROJECT: ProjectMemberList = [
  projectMember("user-amy"),
  projectMember("user-zed"),
  projectMember("user-me"),
  projectMember("user-guest", 5),
];
const DIRECTORY: WorkspaceMemberList = [
  directoryEntry("user-amy", "Amy"),
  directoryEntry("user-zed", "Zed"),
  directoryEntry("user-me", "Me Mine"),
  directoryEntry("user-suspended", "Sam", false),
];

describe("intake member options (INT-009)", () => {
  it("shapes nothing while a read is in flight", () => {
    expect(
      selectIntakeMemberOptions({ projectMembers: undefined, directory: DIRECTORY, meId: null, selectedIds: [] })
    ).toEqual([]);
    expect(
      selectIntakeMemberOptions({ projectMembers: PROJECT, directory: undefined, meId: null, selectedIds: [] })
    ).toEqual([]);
  });

  it("joins project members to the directory and skips guests", () => {
    const options = selectIntakeMemberOptions({
      projectMembers: PROJECT,
      directory: DIRECTORY,
      meId: null,
      selectedIds: [],
    });
    expect(options.map((option) => option.displayName)).toEqual(["Amy", "Me Mine", "Zed"]);
  });

  it("orders selected first, then the signed-in member, then names", () => {
    const options = selectIntakeMemberOptions({
      projectMembers: PROJECT,
      directory: DIRECTORY,
      meId: "user-me",
      selectedIds: ["user-zed"],
    });
    expect(options.map((option) => option.userId)).toEqual(["user-zed", "user-me", "user-amy"]);
  });

  it("marks suspended members from the directory", () => {
    const extended: ProjectMemberList = [...PROJECT, projectMember("user-suspended")];
    const options = selectIntakeMemberOptions({
      projectMembers: extended,
      directory: DIRECTORY,
      meId: null,
      selectedIds: [],
    });
    expect(options.find((option) => option.userId === "user-suspended")?.suspended).toBe(true);
    expect(options.find((option) => option.userId === "user-amy")?.suspended).toBe(false);
  });
});
