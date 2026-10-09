// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-199): long histories page through an explicit
// control that appends older items with a loading label mid-fetch, while
// short streams show no control at all. Row: NTF-010.
//
// Volume comes from the seed (310 read rows for the owner — runtime fan-out
// cannot cover a 300-row page); the scenarios key on counts and the seeded
// oldest title, so reruns on one seeded stack stay green. The short-stream
// branch runs as a freshly provisioned user whose stream holds one item.
// Page sizes stay unpinned: only the control's presence, the loading label
// and the append behavior are asserted.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  serverNotificationsPage,
  serverOnboardSession,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-010"];

test(
  specTitle(ROWS, "long histories page through an explicit control with a loading label"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    await test.step("the control shows while another page exists", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(async () => (await driver.notificationsCards()).length, { timeout: 30_000 }).toBeGreaterThan(0);
      expect(await driver.notificationsNextPageLabel()).not.toBeNull();
    });

    await test.step("activating it shows a loading label and appends older items", async () => {
      const held = await driver.notificationsLoadNextPageHeld(4_000);
      expect(held.loadingShown).toBe(true);
      expect(held.after).toBeGreaterThan(held.before);
    });

    await test.step("exhausting the pages appends down to the oldest seeded item", async () => {
      for (let round = 0; round < 4; round++) {
        if ((await driver.notificationsNextPageLabel()) === null) break;
        const before = (await driver.notificationsCards()).length;
        await driver.notificationsLoadNextPage();
        expect((await driver.notificationsCards()).length).toBeGreaterThan(before);
      }
      expect(await driver.notificationsNextPageLabel()).toBeNull();
      const cards = await driver.notificationsCards();
      expect(cards.length).toBeGreaterThan(0);
      // Every seeded pagination row points at the first seeded issue and
      // predates every runtime row, so the exhausted tail is all seeded.
      expect(cards[cards.length - 1]!.title).toBe(seed.issueNames[0]);
    });

    await test.step("the server pages match the control", async () => {
      const first = await serverNotificationsPage(seed.workspaceSlug, ownerSession, {
        perPage: 5,
        cursor: "5:0:0",
      });
      expect(first.rows).toHaveLength(5);
      expect(first.nextPageResults).toBe(true);
      expect(first.nextCursor).not.toBeNull();
      const second = await serverNotificationsPage(seed.workspaceSlug, ownerSession, {
        perPage: 5,
        cursor: first.nextCursor!,
      });
      expect(second.rows).toHaveLength(5);
      const firstIds = new Set(first.rows.map((row) => row.id));
      expect(second.rows.some((row) => firstIds.has(row.id))).toBe(false);
      const firstOldest = first.rows.map((row) => row.createdAt).sort()[0]!;
      expect(second.rows.every((row) => row.createdAt <= firstOldest)).toBe(true);
    });
  }
);

test(specTitle(ROWS, "short streams show no next-page control"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  const member = requireMentionMember(seed);
  const tag = `ntf010-${Date.now().toString(36)}`;
  const ownerSession = await signInSession(seed.email, seed.password);
  const memberSession = await signInSession(member.email, member.password);

  const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf010", ownerSession);
  try {
    await serverAddProjectMembers(
      seed.workspaceSlug,
      seed.projectId,
      [{ memberId: provisioned.userId, role: 15 }],
      ownerSession
    );
    await serverOnboardSession(provisioned.session);
    const cookies = sessionBrowserCookies(provisioned.session);

    const issueName = `Short stream ${tag}`;
    await test.step("fan one notification out to the fresh user", async () => {
      const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, issueName);
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        issueId,
        provisioned.session,
        `<p>subscribing ${tag}</p>`
      );
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        issueId,
        memberSession,
        `<p>short check ${tag}</p>`
      );
      await expect
        .poll(async () => serverNotificationsList(seed.workspaceSlug, provisioned.session), {
          timeout: 90_000,
        })
        .toHaveLength(1);
    });

    await test.step("one page shows cards and no control", async () => {
      await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
      expect(await driver.notificationsNextPageLabel()).toBeNull();
      const page = await serverNotificationsPage(seed.workspaceSlug, provisioned.session, {
        perPage: 5,
        cursor: "5:0:0",
      });
      // The paginator always renders a cursor string; only the
      // has-results flag tells the client whether to offer the control.
      expect(page.nextPageResults).toBe(false);
    });
  } finally {
    const memberships = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
    const membership = memberships.find((row) => row.userId === provisioned.userId);
    if (membership !== undefined) {
      await serverCleanupWorkspaceMember(seed.workspaceSlug, membership.membershipId, ownerSession);
    }
  }
});
