// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for the issue detail/peek/widget oracle specs (NEWFRONT-121).
// Everything here is written against the driver interface only, so the same
// specs run on apps/web (oracle) and apps/web_new (parity target).
import { expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  PROJECT_ROLE_GUEST,
  addProjectMembers,
  createIssue,
  deactivateUser,
  deleteIssue,
  fetchMe,
  inviteWorkspaceMember,
  issueFacts,
  joinWorkspace,
  listWorkspaceInvites,
  listWorkspaceMembers,
  projectFacts,
  removeWorkspaceMember,
  setOnboarded,
  signInSession,
  signUpUser,
} from "../helpers/api";

/**
 * Sign in and gate on the authenticated session. The shared scratch stack
 * throttles anonymous calls per minute, so under sibling contention the
 * password POST can be dropped while the URL wait still passes; polling the
 * driver-visible session state (with one reload-and-retry) keeps specs
 * honest instead of failing downstream on the sign-in card.
 */
export async function signIn(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  // The shared stack slows down under sibling contention (dev-server
  // compiles, anonymous throttle), so the whole entry sequence retries.
  let lastError: unknown = null;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await expect.poll(() => driver.signedIn(), { timeout: 45_000 }).toBe(true);
      return;
    } catch (error) {
      lastError = error;
      await driver.page.reload().catch(() => {});
    }
  }
  throw lastError;
}

/** A seeded issue resolved live: sequences drift when siblings reset the shared stack. */
export interface SeedIssue {
  seq: string;
  id: string;
  name: string;
}

/** Resolve `seed.issueNames[index]` to its current `IDENT-seq`, id, and name. */
export async function seedIssue(seed: ParitySeedFacts, session: string, index: number): Promise<SeedIssue> {
  const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
  const name = seed.issueNames[index] as string;
  const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session);
  const row = rows.find((r) => r.name === name);
  if (!row) throw new Error(`[parity] seeded issue ${JSON.stringify(name)} is missing (sibling reset?).`);
  return { seq: `${identifier}-${row.sequence_id}`, id: row.id, name };
}

/**
 * Create a scenario-owned issue and resolve its `IDENT-seq`. Owned issues
 * keep mutation scenarios isolated from sibling runs sharing the stack
 * (renames, intake moves, deletes of the seeded rows). Callers delete the
 * issue at the end of the test so sibling exact-set assertions keep passing.
 */
export async function ownIssue(
  seed: ParitySeedFacts,
  session: string,
  name: string,
  extra: Record<string, unknown> = {},
  projectId: string = seed.projectId
): Promise<SeedIssue> {
  const { identifier } = await projectFacts(seed.workspaceSlug, projectId, session);
  const created = await createIssue(seed.workspaceSlug, projectId, session, name, extra);
  return { seq: `${identifier}-${created.sequence_id}`, id: created.id, name };
}

/** Delete a scenario-owned issue (best effort: leftovers are timestamped). */
export async function dropIssue(
  seed: ParitySeedFacts,
  session: string,
  id: string,
  projectId: string = seed.projectId
): Promise<void> {
  await deleteIssue(seed.workspaceSlug, projectId, id, session);
}

/** A scenario-owned second user: credentials, session, and workspace identity. */
export interface GuestUser {
  email: string;
  password: string;
  session: string;
  userId: string;
}

/**
 * Sign up a throwaway guest, join it to the workspace, and add it to the
 * project. Guests need `guest_view_all_features` on the project to read
 * issues at all, so callers pass a scratch project they flipped (the seed
 * project is never touched). Pair with `teardownGuest`.
 */
export async function setupGuest(seed: ParitySeedFacts, ownerSession: string, projectId: string): Promise<GuestUser> {
  const email = `parity-guest-${Date.now()}@example.com`;
  const password = "Parity-Guest-1";
  await signUpUser(email, password);
  const session = await signInSession(email, password);
  await setOnboarded(session);
  await inviteWorkspaceMember(seed.workspaceSlug, ownerSession, email, PROJECT_ROLE_GUEST);
  const invites = await listWorkspaceInvites(seed.workspaceSlug, ownerSession);
  const invite = invites.find((row) => row.email === email);
  if (!invite) throw new Error("[parity] workspace invite for the guest is missing.");
  await joinWorkspace(seed.workspaceSlug, invite.id, invite.token, session);
  const me = await fetchMe(session);
  await addProjectMembers(seed.workspaceSlug, projectId, ownerSession, [
    { member_id: me.id, role: PROJECT_ROLE_GUEST },
  ]);
  return { email, password, session, userId: me.id };
}

/**
 * Remove the guest's workspace membership and deactivate the user. The
 * scratch project (with its project membership) is deleted by the caller.
 * Best effort throughout: teardown must not fail a passing scenario.
 */
export async function teardownGuest(seed: ParitySeedFacts, ownerSession: string, guest: GuestUser): Promise<void> {
  try {
    const members = await listWorkspaceMembers(seed.workspaceSlug, ownerSession);
    const row = members.find((member) => member.email === guest.email);
    if (row) await removeWorkspaceMember(seed.workspaceSlug, row.id, ownerSession);
  } catch {
    // Best effort: a contended stack must not fail teardown.
  }
  try {
    await deactivateUser(guest.session);
  } catch {
    // Best effort: see above.
  }
}
