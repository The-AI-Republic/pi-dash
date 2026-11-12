// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member options for the intake assignee and created-by sections
// (parity/specs/intake.md INT-009). Both sections read the same shared
// picker data — project memberships joined to the workspace directory
// — and order it the intake way: already-selected members first, then
// the signed-in member, then everyone else by display name. Rows always
// show display names: the shared picker's "You" label belongs to its
// trigger, not to a filter row or chip.
//
// The spec's reuse map suggests the shared MemberPicker for created-by;
// both sections use inline checkbox rows instead. A popup trigger inside
// the panel would give the two member sections different behavior and a
// different order (the picker leads with the signed-in member), while
// INT-009 requires the same member list, the same checkbox behavior and
// the selected-first order on both. The data layer stays shared.

import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";

import { selectMemberOptions } from "../../../shared/pickers/index.js";

/** One member row the intake filter sections can offer, in order. */
export interface IntakeMemberOption {
  userId: string;
  displayName: string;
  avatarUrl: string | null;
  suspended: boolean;
}

export interface IntakeMemberOptionsInput {
  projectMembers: ProjectMemberList | undefined;
  directory: WorkspaceMemberList | undefined;
  /** Signed-in user id, or null while the session resolves. */
  meId: string | null;
  /** Ids selected in the section being shaped. */
  selectedIds: readonly string[];
}

/**
 * The section's member rows: the shared option set, reordered to
 * selected first, the signed-in member next, then display-name order.
 * Reads still in flight shape to no rows; the section shows its
 * loading state instead.
 */
export function selectIntakeMemberOptions(input: IntakeMemberOptionsInput): IntakeMemberOption[] {
  if (input.directory === undefined || input.projectMembers === undefined) return [];
  const shared = selectMemberOptions({
    projectMembers: input.projectMembers,
    directory: input.directory,
    meId: input.meId,
    selectedIds: input.selectedIds,
  });
  const selected = new Set(input.selectedIds);
  const shaped = shared.map((option) => ({
    userId: option.userId,
    displayName: option.searchText,
    avatarUrl: option.avatarUrl,
    suspended: option.suspended,
  }));
  // Selected first, the signed-in member next, then everyone else; ties
  // break on display name. The identity comparison is null-safe, so an
  // unresolved session simply ranks nobody second.
  const rank = (option: IntakeMemberOption): number => {
    if (selected.has(option.userId)) return 0;
    if (option.userId === input.meId) return 1;
    return 2;
  };
  shaped.sort((left, right) => rank(left) - rank(right) || left.displayName.localeCompare(right.displayName));
  return shaped;
}
