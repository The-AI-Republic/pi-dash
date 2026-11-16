// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member option shaping (parity/specs/issues.md §17, ISS-208). Project
// memberships carry bare user ids, so the picker joins them to the
// workspace directory for display names and avatars; without a project
// scope the directory itself is the option source. Guests are never
// offered. Deactivated directory entries render suspended. Sorting puts
// the signed-in member first, then the selected members.
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";

import { ROLE_GUEST } from "../../core/session/permissions.js";

/** One member row the picker can offer, in display order. */
export interface MemberOption {
  userId: string;
  /** "You" for the signed-in member, else the display name. */
  label: string;
  /** Display name behind the label, so typing it still matches. */
  searchText: string;
  avatarUrl: string | null;
  suspended: boolean;
  selected: boolean;
}

export interface MemberOptionsInput {
  /** Bare project memberships; absent in workspace scope. */
  projectMembers?: ProjectMemberList | undefined;
  directory: WorkspaceMemberList;
  /** Signed-in user id, or null while the session resolves. */
  meId: string | null;
  selectedIds: readonly string[];
}

interface DirectoryEntry {
  displayName: string;
  avatarUrl: string | null;
  role: number;
  suspended: boolean;
}

function toOption(
  userId: string,
  entry: DirectoryEntry | undefined,
  meId: string | null,
  selectedIds: readonly string[]
): MemberOption {
  const displayName = entry?.displayName ?? "Unknown member";
  return {
    userId,
    label: meId !== null && userId === meId ? "You" : displayName,
    searchText: displayName,
    avatarUrl: entry?.avatarUrl ?? null,
    suspended: entry?.suspended ?? false,
    selected: selectedIds.includes(userId),
  };
}

function byPickerOrder(meId: string | null) {
  return (left: MemberOption, right: MemberOption): number => {
    if (meId !== null) {
      if (left.userId === meId && right.userId !== meId) return -1;
      if (right.userId === meId && left.userId !== meId) return 1;
    }
    if (left.selected && !right.selected) return -1;
    if (right.selected && !left.selected) return 1;
    return 0;
  };
}

export function selectMemberOptions(input: MemberOptionsInput): MemberOption[] {
  const directory = new Map<string, DirectoryEntry>();
  for (const row of input.directory) {
    if (row.role === ROLE_GUEST) continue;
    directory.set(row.member.id, {
      displayName: row.member.display_name,
      avatarUrl: row.member.avatar_url,
      role: row.role,
      suspended: row.is_active === false,
    });
  }
  const candidates =
    input.projectMembers === undefined
      ? [...directory.entries()].map(([userId, entry]) => toOption(userId, entry, input.meId, input.selectedIds))
      : input.projectMembers
          .filter((row) => row.role !== ROLE_GUEST)
          .map((row) => toOption(row.member, directory.get(row.member), input.meId, input.selectedIds));
  candidates.sort(byPickerOrder(input.meId));
  return candidates;
}
