// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member row actions and the shared confirmation copy (parity/specs/
// workspace-settings.md §12–§14, WSET-012/013/015). Which menu entry a row
// offers, whether its account type is editable, and the confirm dialog's
// title and body all derive from the actor, the row and the row's owner.
// One dialog serves member removal, self-leave and invitation withdrawal;
// its title names what leaves.

import type { WorkspaceInvitation, WorkspaceRosterMember } from "@pidash/api-client";

import { isSuspended } from "./roster.js";

export type MemberMenuAction = "remove" | "leave";

/** Menu entry for a row: admins remove others, anyone leaves their own; suspended rows offer none. */
export function memberMenuAction(
  row: WorkspaceRosterMember,
  actorUserId: string | null,
  isAdmin: boolean
): MemberMenuAction | null {
  if (isSuspended(row)) return null;
  if (actorUserId !== null && row.member.id === actorUserId) return "leave";
  return isAdmin ? "remove" : null;
}

/**
 * Whether the account-type control renders (§12): admins only, on active
 * rows, never on the actor's own row.
 */
export function memberRoleEditable(row: WorkspaceRosterMember, actorUserId: string | null, isAdmin: boolean): boolean {
  if (!isAdmin || isSuspended(row)) return false;
  return actorUserId === null || row.member.id !== actorUserId;
}

/** Title and body of the shared remove/leave confirmation (§13). */
export interface RemoveDialogCopy {
  title: string;
  body: string;
  confirm: string;
}

export type RemoveTarget =
  | { kind: "member"; row: WorkspaceRosterMember }
  | { kind: "self" }
  | { kind: "invitation"; invitation: WorkspaceInvitation };

/** Name shown in the member body: the display name, falling back to the address. */
export function memberDisplayName(row: WorkspaceRosterMember): string {
  return row.member.display_name.trim() === "" ? row.member.email : row.member.display_name;
}

export function removeDialogCopy(target: RemoveTarget): RemoveDialogCopy {
  if (target.kind === "invitation") {
    return {
      title: `Remove ${target.invitation.email}?`,
      body: `This withdraws the pending invitation for ${target.invitation.email}. The address can be invited again later.`,
      confirm: "Remove",
    };
  }
  if (target.kind === "self") {
    return {
      title: "Leave workspace?",
      body: "You will lose access to this workspace and its projects. A workspace admin must invite you again to bring you back.",
      confirm: "Leave",
    };
  }
  const name = memberDisplayName(target.row);
  return {
    title: `Remove ${name}?`,
    body: `${name} will no longer have access to this workspace. Their work stays behind, but they lose access irreversibly until an admin invites them again.`,
    confirm: "Remove",
  };
}
