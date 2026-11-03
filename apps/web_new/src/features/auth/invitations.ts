// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Invitation inbox and single-invitation decisions (AUTH-026–028). Written
// from parity/specs/auth.md §§AUTH-026–028: card selection, invited-role
// labels, the single-link view state machine, and post-answer landing.
// Pure, so the inbox page, the emailed-link page, and the onboarding
// workspace step (which reuses these contracts with embedded views) share
// them; page chrome stays in the components.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  type ApiClient,
  type WorkspaceInvitation,
  type WorkspaceInvitationList,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { ROLE_ADMIN, ROLE_MEMBER } from "../../core/session/permissions.js";
import { authKeys } from "./queries.js";

/** The invited role as a label (backend roles: Admin 20, Member 15, Guest 5). */
export function selectInvitationRoleLabel(role: number): string {
  if (role >= ROLE_ADMIN) return "Admin";
  if (role >= ROLE_MEMBER) return "Member";
  return "Guest";
}

/** Toggle one invite id in the inbox selection, preserving click order. */
export function toggleInvitationSelection(selected: string[], id: string): string[] {
  return selected.includes(id) ? selected.filter((item) => item !== id) : [...selected, id];
}

/**
 * Batch-accept landing target (AUTH-026/028): the workspace of the first
 * selected invite that still resolves, in selection order. Null when no
 * selection resolves — the caller fails closed instead of landing on a
 * workspace the invite list no longer names.
 */
export function selectBatchLandingTarget(
  invitations: WorkspaceInvitationList,
  selectedIds: string[]
): { id: string; slug: string } | null {
  for (const id of selectedIds) {
    const invite = invitations.find((candidate) => candidate.id === id);
    if (invite !== undefined) return { id: invite.workspace.id, slug: invite.workspace.slug };
  }
  return null;
}

/**
 * Single-invite accept landing (AUTH-028): the joined workspace when the
 * signed-in address matches the invitee address, else the home route.
 * The comparison is exact, matching the old page's identity check.
 */
export function resolveSingleAcceptLanding(args: {
  inviteeEmail: string;
  viewerEmail: string | undefined;
  slug: string;
}): string {
  return args.viewerEmail === args.inviteeEmail ? `/${args.slug}` : "/";
}

/** Ignoring always lands on the home route (AUTH-028). */
export const INVITATION_IGNORE_LANDING = "/";

export type SingleInvitationViewState = "pending" | "already-member" | "inactive" | "not-found" | "loading";

export type SingleInvitationView =
  | { state: "pending"; invitation: WorkspaceInvitation }
  | { state: "already-member"; invitation: WorkspaceInvitation }
  | { state: "inactive" }
  | { state: "not-found" }
  | { state: "loading" };

/**
 * Single-link view state machine (AUTH-027). A pending invite renders the
 * accept/ignore actions; an answered-plus-accepted invite renders the
 * already-member card (reachable only when the invite was answered before
 * any account existed for its address); an answered-plus-declined invite
 * or a fetch failure renders the inactive card, as does an unresolvable
 * id. The not-found branch needs fetched detail plus a fetch error at
 * once, which a fresh broken link never produces, so it stays effectively
 * dead — implemented and unit-tested, never rendered in practice.
 */
export function decideSingleInvitationView(args: {
  invitation: WorkspaceInvitation | null;
  failed: boolean;
}): SingleInvitationView {
  const { invitation, failed } = args;
  if (invitation !== null && invitation.responded_at === null) {
    return failed ? { state: "not-found" } : { state: "pending", invitation };
  }
  if (failed || invitation !== null) {
    if (invitation?.accepted === true) {
      return { state: "already-member", invitation };
    }
    return { state: "inactive" };
  }
  return { state: "loading" };
}

/** One emailed invitation for the invitation-aware card header (AUTH-005). */
export function invitationQueryOptions(client: ApiClient, slug: string, invitationId: string) {
  return queryOptions({
    queryKey: authKeys.invitation(slug, invitationId),
    queryFn: () => getWorkspaceInvitation(client, slug, invitationId),
    enabled: slug.length > 0 && invitationId.length > 0,
  });
}

/** The emailed invitation when the card URL carries one; disabled otherwise. */
export function useWorkspaceInvitation(slug?: string | undefined, invitationId?: string | undefined) {
  return useQuery(invitationQueryOptions(getAppClient(), slug ?? "", invitationId ?? ""));
}

/** The caller's own workspace invitations (AUTH-026 inbox, AUTH-029 progress). Reusable across the area. */
export function myInvitationsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: authKeys.myInvitations,
    queryFn: () => listWorkspaceInvitations(client),
  });
}

/** The caller's own workspace invitations. */
export function useMyWorkspaceInvitations() {
  return useQuery(myInvitationsQueryOptions(getAppClient()));
}

/** The caller's own workspace join requests (AUTH-034 holding view). Reusable across the area. */
export function myJoinRequestsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: authKeys.myJoinRequests,
    queryFn: () => listWorkspaceJoinRequests(client),
  });
}

/** The caller's own workspace join requests. */
export function useMyWorkspaceJoinRequests() {
  return useQuery(myJoinRequestsQueryOptions(getAppClient()));
}
