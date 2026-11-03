// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-invitations rules (parity/specs/workspace-settings.md §11,
// WSET-010). The group lists the invitations the roster already reads,
// narrowed by the same search box (address only) and ordered by their
// arrival. Copying a link resolves the stored path against the running
// site; guilt-free when the invitation carries none.

import type { WorkspaceInvitation } from "@pidash/api-client";

import { invitationMatchesSearch } from "./roster.js";

/** Group heading; the oracle finds the toggle by this text. */
export const PENDING_GROUP_LABEL = "Pending invites";

/** The invitations the search leaves, oldest first. */
export function selectPendingInvitations(
  invitations: readonly WorkspaceInvitation[],
  search: string
): WorkspaceInvitation[] {
  return invitations
    .filter((invitation) => invitationMatchesSearch(invitation, search))
    .slice()
    .sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at));
}

/** Count shown beside the heading: the search-matching invitations. */
export function pendingInvitationCount(invitations: readonly WorkspaceInvitation[], search: string): number {
  return selectPendingInvitations(invitations, search).length;
}

/**
 * The absolute address to copy for one invitation: the stored link
 * resolved against the running origin. Null when the row carries no
 * link, in which case the Copy entry is not offered.
 */
export function invitationLink(invitation: WorkspaceInvitation, origin: string): string | null {
  const stored = invitation.invite_link.trim();
  if (stored === "") return null;
  try {
    return new URL(stored, origin).href;
  } catch {
    return null;
  }
}

/** Address used for the invitation avatar's initial. */
export function invitationInitial(email: string): string {
  const first = email.trim().charAt(0).toUpperCase();
  return first === "" ? "?" : first;
}

/** The remove confirmation's heading for one invitation (§11). */
export function invitationWithdrawTitle(invitation: WorkspaceInvitation): string {
  return `Remove ${invitation.email}?`;
}

/**
 * Copy one invitation's absolute link (§11). A row without a link asks for
 * nothing; the clipboard's refusal stays silent, and a write that lands
 * confirms with the copied notice. The host supplies its origin so the
 * module stays pure and testable.
 */
export function copyInvitationLink(
  invitation: WorkspaceInvitation,
  origin: string,
  clipboard: Pick<Clipboard, "writeText">,
  onCopied: () => void
): void {
  const link = invitationLink(invitation, origin);
  if (link === null) return;
  void clipboard.writeText(link).then(onCopied, () => undefined);
}
