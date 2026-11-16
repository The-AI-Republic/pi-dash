// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition entry for the workspace-settings rows that carry a cloud leg
// (WSET-015, WSET-029-031). OSS seeds carry a password and sign in through
// the credential form; the cloud seed has none and enters through the
// provider's dev-login endpoint, exactly like the cloud area's own
// scenarios (NEWFRONT-436). Callers stay edition-agnostic after entry: the
// rows pin the same screens and rules on both builds.
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  cloudBrowserCookies,
  cloudDevLogin,
  cloudProvisionWorkspaceMember,
  serverCompleteOnboarding,
  serverPatchWorkspaceMemberRole,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembersFull,
  sessionBrowserCookies,
  signInSession,
  uniqueEmail,
  type CloudSession,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";

/** Whether this run points the suite at the cloud-edition stack. */
export function isCloudEdition(env: Record<string, string | undefined> = process.env): boolean {
  return (env["PARITY_ORACLE_EDITION"] ?? "").trim().toLowerCase() === "cloud";
}

/** A seed that enters through the credential form (OSS); blank means cloud. */
function hasPassword(seed: ParitySeedFacts): boolean {
  const password: unknown = seed.password;
  return typeof password === "string" && password !== "";
}

/** Server session for a seeded identity, in this edition's way. */
export async function seededSession(seed: ParitySeedFacts, email: string): Promise<string> {
  if (!isCloudEdition() && hasPassword(seed)) return signInSession(email, seed.password);
  return (await cloudDevLogin(email)).cookie;
}

/**
 * Identity a scenario wants on screen: the server session plus the
 * credential-form password OSS runs sign in with (absent on cloud builds,
 * which have no password).
 */
export interface EditionUser {
  email: string;
  session: string;
  password?: string;
}

/**
 * Put the browser on a workspace page as `user`. A cloud build renders no
 * credential form, so those runs install the provider cookies and navigate
 * directly; OSS runs use the sign-in page and its remembered workspace.
 */
export async function openWorkspacePage(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  path: string,
  user: EditionUser
): Promise<void> {
  if (!isCloudEdition() && user.password !== undefined && user.password !== "") {
    await driver.rulesEnsureSignedIn(user.email, user.password, seed.workspaceSlug);
    return;
  }
  const cookies = cloudBrowserCookies(user.session).map((cookie) => ({ ...cookie, domain: "localhost" }));
  await driver.openAuthenticated(path, cookies);
}

/** Browser cookies for a session, shaped for the edition at hand. */
export function browserSessionCookies(session: string, seed: ParitySeedFacts) {
  return !isCloudEdition() && hasPassword(seed) ? sessionBrowserCookies(session) : cloudBrowserCookies(session);
}

export interface ManagedMember {
  userId: string;
  membershipId: string;
  session: string;
  email: string;
  /** Credential-form password (OSS only). */
  password?: string;
  displayName: string;
}

/** Members on the workspace, read through the edition's own API host. */
async function memberRows(seed: ParitySeedFacts, ownerSession: string) {
  return serverWorkspaceMembersFull(seed.workspaceSlug, ownerSession);
}

/**
 * Provision a fresh workspace member at `role`, onboarded and ready to
 * browse settings. OSS users register with a password; cloud identities
 * arrive through dev-login and an invitation join.
 */
export async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string,
  role: number
): Promise<ManagedMember> {
  let userId: string;
  let email: string;
  let session: string;
  let password: string | undefined;
  if (isCloudEdition()) {
    email = uniqueEmail(prefix);
    const cloud: CloudSession = await cloudProvisionWorkspaceMember(seed.workspaceSlug, email, role, ownerSession);
    userId = cloud.userId;
    session = cloud.cookie;
  } else {
    const created = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
    userId = created.userId;
    email = created.email;
    session = created.session;
    password = created.password;
    await serverCompleteOnboarding(session);
  }
  let row = (await memberRows(seed, ownerSession)).find((entry) => entry.userId === userId);
  if (row === undefined) throw new Error("[parity] provisioned member missing from workspace list.");
  if (row.role !== role) {
    await serverPatchWorkspaceMemberRole(seed.workspaceSlug, row.membershipId, role, ownerSession);
    row = (await memberRows(seed, ownerSession)).find((entry) => entry.userId === userId);
    if (row === undefined) throw new Error("[parity] provisioned member missing from workspace list.");
  }
  return { userId, membershipId: row.membershipId, session, email, password, displayName: row.displayName };
}

/** Complete the first-run tour flag so it never covers the workspace sidebar. */
export async function markTourCompleted(session: string): Promise<void> {
  const apiBase = parityApiUrl(process.env).trim().replace(/\/+$/, "");
  const res = await fetch(`${apiBase}/api/users/me/tour-completed/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: session },
    body: JSON.stringify({ is_tour_completed: true }),
  });
  if (!res.ok) throw new Error(`[parity] tour flag PATCH failed with HTTP ${res.status}.`);
}
