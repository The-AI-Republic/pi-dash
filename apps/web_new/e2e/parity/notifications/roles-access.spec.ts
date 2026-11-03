// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-202): every workspace role opens the same
// inbox, acts on cards, and edits its own email preferences. Row: NTF-026.
//
// One test per role; each signs in fresh through the UI. The member and
// guest notifications fan out from an explicit API subscription plus a
// comment by another user (the same trigger the siblings' comment fan-out
// relies on). The guest reads through a dedicated project that opts into
// guest-visible issues, because the seeded project refuses guest issue
// reads and unresolvable cards are skipped (NTF-005).
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  parityApiBase,
  parityProjectIdentifier,
  requireMentionMember,
  serverAddProjectMembers,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateComment,
  serverCreateIssue,
  serverCreateProjectWithFlags,
  serverEmailPreferences,
  serverNotificationsList,
  serverPatchProject,
  serverProjectMembers,
  serverRequestStatus,
  signInSession,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-026"];

const PREF = "comment" as const;

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

async function subscribeSelf(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string
): Promise<void> {
  const res = await serverRequestStatus(
    "POST",
    `${parityApiBase()}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/subscribe/`,
    sessionCookie
  );
  // 400 means already subscribed (a rerun colliding on one issue); the
  // subscription the fan-out needs is in place either way.
  if (res.status !== 201 && res.status !== 400)
    throw new Error(`[parity] subscribe failed with HTTP ${res.status}: ${res.bodyText}`);
}

async function fanOutToSubscriber(
  workspaceSlug: string,
  projectId: string,
  issueName: string,
  subscriberSession: string,
  commenterSession: string
): Promise<string> {
  const issueId = await serverCreateIssue(workspaceSlug, projectId, commenterSession, issueName);
  await subscribeSelf(workspaceSlug, projectId, issueId, subscriberSession);
  await serverCreateComment(workspaceSlug, projectId, issueId, commenterSession, `<p>notable ${issueName}</p>`);
  await expect
    .poll(
      async () => {
        const rows = await serverNotificationsList(workspaceSlug, subscriberSession);
        return rows.some((row) => row.entityIdentifier === issueId);
      },
      { timeout: 90_000 }
    )
    .toBe(true);
  return issueId;
}

async function proveRoleAccess(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  email: string,
  password: string,
  sessionCookie: string,
  issueName: string
): Promise<void> {
  await driver.rulesEnsureSignedIn(email, password, seed.workspaceSlug);

  await test.step("the role opens the same two-pane inbox", async () => {
    await driver.notificationsOpenInbox(seed.workspaceSlug);
    expect(await driver.notificationsListPaneVisible()).toBe(true);
    expect(await driver.notificationsDetailPaneVisible()).toBe(true);
    await expect
      .poll(async () => (await driver.notificationsCards()).map((card) => card.title), { timeout: 30_000 })
      .toContain(issueName);
  });

  await test.step("the role acts on a card: select opens detail and marks it read", async () => {
    const titles = (await driver.notificationsCards()).map((card) => card.title);
    await driver.notificationsSelectCard(titles.indexOf(issueName));
    expect(await driver.notificationsDetailVariant()).not.toBe("placeholder");
    await expect
      .poll(
        async () =>
          (await serverNotificationsList(seed.workspaceSlug, sessionCookie)).find((row) => row.issueName === issueName)
            ?.readAt ?? null,
        { timeout: 30_000 }
      )
      .not.toBeNull();
  });

  await test.step("the role edits its own email preferences with instant save", async () => {
    await driver.notificationsOpenEmailPreferences();
    const before = (await serverEmailPreferences(sessionCookie))[PREF];
    await driver.notificationsEmailPreferencesToggle(PREF);
    await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
    expect((await driver.notificationsEmailPreferences())[PREF]).toBe(!before);
    expect((await serverEmailPreferences(sessionCookie))[PREF]).toBe(!before);
    await driver.notificationsEmailPreferencesToggle(PREF);
    expect((await driver.notificationsEmailPreferences())[PREF]).toBe(before);
    expect((await serverEmailPreferences(sessionCookie))[PREF]).toBe(before);
  });
}

test(
  specTitle(ROWS, "admin opens the inbox, acts on cards, and edits email preferences"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf026a-${Date.now().toString(36)}`;
    const issueName = `Role admin ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    await fanOutToSubscriber(seed.workspaceSlug, seed.projectId, issueName, ownerSession, memberSession);
    await proveRoleAccess(driver, seed, seed.email, seed.password, ownerSession, issueName);
  }
);

test(
  specTitle(ROWS, "member opens the inbox, acts on cards, and edits email preferences"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf026m-${Date.now().toString(36)}`;
    const issueName = `Role member ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    await fanOutToSubscriber(seed.workspaceSlug, seed.projectId, issueName, memberSession, ownerSession);
    await proveRoleAccess(driver, seed, member.email, member.password, memberSession, issueName);
  }
);

test(
  specTitle(ROWS, "guest opens the inbox, acts on cards, and edits email preferences"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf026g-${Date.now().toString(36)}`;
    const issueName = `Role guest ${tag}`;
    const guestCreds = requireGuest(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const guestSession = await signInSessionRetry(guestCreds.email, guestCreds.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      // Project names reject special characters (dashes included).
      `NTF026 guest ${Date.now().toString(36)} project`,
      parityProjectIdentifier("N202"),
      {},
      ownerSession
    );
    let issueId = "";
    try {
      await serverPatchProject(seed.workspaceSlug, projectId, ownerSession, { guest_view_all_features: true });
      const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, ownerSession);
      const guestUserId = members.find((row) => row.role === 5)?.userId;
      if (!guestUserId) throw new Error("[parity] seed project has no role-5 guest member.");
      await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestUserId, role: 5 }], ownerSession);
      issueId = await fanOutToSubscriber(seed.workspaceSlug, projectId, issueName, guestSession, ownerSession);
      await proveRoleAccess(driver, seed, guestCreds.email, guestCreds.password, guestSession, issueName);
    } finally {
      if (issueId !== "") await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issueId, ownerSession);
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);
