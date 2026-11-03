// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for the agents-area oracle scenarios (NEWFRONT-184). Every
// scenario builds a fresh owner plus a fresh workspace, so runs stay
// convergent (unique slugs) and isolated from sibling runs sharing the
// scratch stack. Sibling agents-area children reuse this harness.
import type { FreshUser } from "../helpers/api";
import {
  ROLE,
  addProjectMembers,
  createWorkspaceViaApi,
  ensureWorkspaceMember,
  markOnboarded,
  serverSessionUserId,
  setLastWorkspace,
  signUpFreshUser,
  uniqueEmail,
  uniqueSlug,
} from "../helpers/api";

/** A scenario-owned workspace plus its owner. */
export interface SchedulerHarness {
  owner: FreshUser;
  /** API session cookie for the owner (the sign-up session). */
  ownerSession: string;
  workspaceSlug: string;
  workspaceName: string;
  workspaceId: string;
  /** The slug with hyphens stripped, for hyphen-hostile fields (project names). */
  tag: string;
}

/** Fresh owner plus fresh workspace; the owner is onboarded and lands there. */
export async function schedulerHarness(prefix = "parity-agt"): Promise<SchedulerHarness> {
  const owner = await signUpFreshUser(prefix);
  const workspaceSlug = uniqueSlug("pwagt");
  const workspaceName = `Parity AGT ${workspaceSlug}`;
  const workspace = await createWorkspaceViaApi(owner, { name: workspaceName, slug: workspaceSlug });
  await markOnboarded(owner);
  await setLastWorkspace(owner, workspace.id);
  return {
    owner,
    ownerSession: owner.cookie,
    workspaceSlug,
    workspaceName,
    workspaceId: workspace.id,
    tag: workspaceSlug.replace(/-/g, ""),
  };
}

/** Password for harness-seated members and guests (meets the signup rules). */
export const HARNESS_MEMBER_PASSWORD = "Parity-Agt-9x";

/**
 * Seat a fresh user on the harness workspace with `role` (member or guest)
 * and return UI credentials plus an API session. Convergent: re-seating an
 * already-seated email returns the existing membership.
 */
export async function seatWorkspaceRole(
  harness: SchedulerHarness,
  role: number,
  prefix = "parity-agtm"
): Promise<{ email: string; password: string; session: string }> {
  const email = uniqueEmail(prefix);
  const session = await ensureWorkspaceMember(
    harness.workspaceSlug,
    harness.ownerSession,
    email,
    HARNESS_MEMBER_PASSWORD,
    role
  );
  return { email, password: HARNESS_MEMBER_PASSWORD, session };
}

/** Seat a workspace member (role 15) on the harness workspace. */
export async function seatMember(harness: SchedulerHarness): Promise<{
  email: string;
  password: string;
  session: string;
}> {
  return seatWorkspaceRole(harness, ROLE.MEMBER, "parity-agtm");
}

/** Seat a workspace guest (role 5) on the harness workspace. */
export async function seatGuest(harness: SchedulerHarness): Promise<{
  email: string;
  password: string;
  session: string;
}> {
  return seatWorkspaceRole(harness, ROLE.GUEST, "parity-agtg");
}

/**
 * Seat a fresh user on the harness workspace with `workspaceRole`, then add
 * them to `projectId` with `projectRole` (project standing is separate from
 * workspace standing), and return UI credentials plus an API session and the
 * user id. Convergent: every call mints a fresh identity.
 */
export async function seatProjectRole(
  harness: SchedulerHarness,
  projectId: string,
  workspaceRole: number,
  projectRole: number,
  prefix = "parity-agtp"
): Promise<{ email: string; password: string; session: string; userId: string }> {
  const seat = await seatWorkspaceRole(harness, workspaceRole, prefix);
  const userId = await serverSessionUserId(seat.session);
  await addProjectMembers(harness.workspaceSlug, projectId, harness.ownerSession, [
    { member_id: userId, role: projectRole },
  ]);
  return { ...seat, userId };
}

/**
 * A fully onboarded user with no seat on the harness workspace, for the
 * route-gate refusal halves. They own an unrelated workspace so sign-in
 * lands somewhere real.
 */
export async function outsiderUser(): Promise<{ email: string; password: string }> {
  const outsider = await signUpFreshUser("parity-agto");
  const slug = uniqueSlug("pwagto");
  const workspace = await createWorkspaceViaApi(outsider, { name: `Parity AGT outsider ${slug}`, slug });
  await markOnboarded(outsider);
  await setLastWorkspace(outsider, workspace.id);
  return { email: outsider.email, password: outsider.password };
}
