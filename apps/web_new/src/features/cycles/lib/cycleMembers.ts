// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle row memberships (parity/specs/cycles.md §2, CYC-002). Rows
// carry bare user ids for the creator and the assignees; the workspace
// directory resolves them to display names and avatars. Ids missing
// from the directory render a stable fallback instead of an empty
// cluster.

import type { WorkspaceMemberList } from "@pidash/api-client";

export interface CycleMemberEntry {
  displayName: string;
  avatarUrl: string | null;
}

/** Fallback for user ids the directory does not know. */
export const UNKNOWN_CYCLE_MEMBER: CycleMemberEntry = {
  displayName: "Unknown member",
  avatarUrl: null,
};

/** Workspace directory keyed by user id for the row avatars. */
export function selectCycleMemberDirectory(directory: WorkspaceMemberList): Map<string, CycleMemberEntry> {
  const members = new Map<string, CycleMemberEntry>();
  for (const row of directory) {
    members.set(row.member.id, {
      displayName: row.member.display_name,
      avatarUrl: row.member.avatar_url,
    });
  }
  return members;
}
