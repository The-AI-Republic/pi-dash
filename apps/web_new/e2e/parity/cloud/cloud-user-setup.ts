// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provisioned cloud users for the profile-tabs oracle scenarios
// (NEWFRONT-440, CLOUD-032..040). Fresh dev-login users land on
// onboarding and hold no workspace, so UI scenarios enter through an
// onboarded user — a workspace member where the surface is
// workspace-scoped — while API-only scenarios use the bare dev-login.
// Local to this area: every step converges, and user rows belong to the
// scratch stack and vanish with it.
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  cloudDevLogin,
  inviteWorkspaceMember,
  joinWorkspace,
  listWorkspaceInvites,
  serverWorkspaceMembers,
  uniqueEmail,
  type CloudSession,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";

/** Mark a cloud user onboarded through the app's own endpoint. */
export async function cloudMarkOnboarded(sessionCookie: string): Promise<void> {
  const apiBase = parityApiUrl(process.env).trim().replace(/\/+$/, "");
  const res = await fetch(`${apiBase}/api/users/me/onboard/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ is_onboarded: true }),
  });
  if (!res.ok) throw new Error(`[parity] cloud onboard PATCH failed with HTTP ${res.status}.`);
}

/** Fresh onboarded cloud user with no workspace (user-level surfaces). */
export async function cloudOnboardedUser(
  prefix: string,
  displayName?: string,
  email: string = uniqueEmail(prefix)
): Promise<CloudSession> {
  const session = await cloudDevLogin(email, displayName);
  await cloudMarkOnboarded(session.cookie);
  return session;
}

/** Fresh onboarded cloud user holding `role` on the seed workspace. */
export async function cloudWorkspaceUser(
  seed: ParitySeedFacts,
  prefix: string,
  role = 15,
  displayName?: string
): Promise<CloudSession> {
  const apiBase = parityApiUrl(process.env).trim().replace(/\/+$/, "");
  const owner = await cloudDevLogin(seed.email);
  const email = uniqueEmail(prefix);
  const session = await cloudDevLogin(email, displayName, apiBase);
  const members = await serverWorkspaceMembers(seed.workspaceSlug, owner.cookie, apiBase);
  if (!members.some((row) => row.userId === session.userId)) {
    await inviteWorkspaceMember(seed.workspaceSlug, owner.cookie, email, role, apiBase).catch(() => undefined);
    const invites = await listWorkspaceInvites(seed.workspaceSlug, owner.cookie, apiBase);
    const invite = invites.find((row) => row.email === email);
    if (invite === undefined) throw new Error(`[parity] cloud invite for ${email} left no pending row.`);
    await joinWorkspace(seed.workspaceSlug, invite.id, invite.token, session.cookie, apiBase);
  }
  await cloudMarkOnboarded(session.cookie);
  return session;
}
