// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-198): the inbox stream tabs and their badges,
// the navigation unread signal, and the unread-count endpoint behind both.
// Each tab carries its own unread badge (hidden at zero); switching tabs
// discards the current list and loads the matching stream. Rows: NTF-002,
// NTF-003, NTF-004.
//
// NTF-003 is a bug row (NEWFRONT-204): the intended navigation count
// badge (mentions count with its marker, else the total) never renders —
// its sidebar branch is unreachable — and the top-bar inbox icon shows
// only a dot. The second scenario pins that actual behavior instead.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out (a plain comment for the full stream, a mention for
// the mentions stream), then poll the server for the unseen rows, so
// reruns on one seeded stack stay green. Zero-state branches run as a
// freshly provisioned user whose inbox starts empty.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverCreateComment,
  serverCreateIssue,
  serverMe,
  serverMentionHtml,
  serverNotificationsList,
  serverNotificationsUnread,
  serverOnboardSession,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const TAB_ROWS = ["NTF-002", "NTF-004"];
const NAV_ROWS = ["NTF-003", "NTF-004"];

test(
  specTitle(TAB_ROWS, "stream tabs switch lists with per-tab unread badges"),
  { tag: specTags(TAB_ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf002-${Date.now().toString(36)}`;
    const plainName = `Tab plain ${tag}`;
    const mentionName = `Tab mention ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const owner = await serverMe(ownerSession);
    const unreadBefore = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);

    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, plainName);
    const mentionId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, mentionName);
    await test.step("fan out one plain and one mention notification", async () => {
      await serverCreateComment(seed.workspaceSlug, seed.projectId, plainId, ownerSession, `<p>subscribing ${tag}</p>`);
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        plainId,
        memberSession,
        `<p>stream check ${tag}</p>`
      );
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        mentionId,
        memberSession,
        serverMentionHtml(owner.id, owner.displayName, `mention check ${tag}`)
      );
      await expect
        .poll(
          async () => {
            const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
            return rows.some((row) => row.entityIdentifier === plainId && !row.isMentioned);
          },
          { timeout: 90_000 }
        )
        .toBe(true);
      await expect
        .poll(
          async () => {
            const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession, { mentioned: true });
            return rows.some((row) => row.entityIdentifier === mentionId && row.isMentioned);
          },
          { timeout: 90_000 }
        )
        .toBe(true);
    });

    await test.step("the unread endpoint counts the fan-out", async () => {
      const unread = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(unread.total - unreadBefore.total).toBe(1);
      expect(unread.mentions - unreadBefore.mentions).toBe(1);
    });

    await test.step("the full stream shows the plain item with its own badge", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      expect(await driver.notificationsTabNames()).toHaveLength(2);
      expect(await driver.notificationsActiveTab()).toBe("all");
      const unread = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      // Badges populate once the entry fetch lands; poll to the server totals.
      await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe(String(unread.total));
      // The list fetch trails the badge fetch; poll to our card.
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(plainName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(mentionName);
    });

    await test.step("switching to mentions discards the list and loads that stream", async () => {
      await driver.notificationsSelectTab("mentions");
      expect(await driver.notificationsActiveTab()).toBe("mentions");
      const unread = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      await expect
        .poll(() => driver.notificationsTabBadge("mentions"), { timeout: 30_000 })
        .toBe(String(unread.mentions));
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(mentionName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(plainName);
    });

    await test.step("each tab mirrors its server query", async () => {
      const full = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(full.every((row) => !row.isMentioned)).toBe(true);
      expect(full.some((row) => row.entityIdentifier === plainId)).toBe(true);
      const mentions = await serverNotificationsList(seed.workspaceSlug, ownerSession, {
        mentioned: true,
      });
      expect(mentions.every((row) => row.isMentioned)).toBe(true);
      expect(mentions.some((row) => row.entityIdentifier === mentionId)).toBe(true);
    });
  }
);

test(
  specTitle(NAV_ROWS, "bug: NEWFRONT-204 navigation shows only a dot, never the unread count badge"),
  { tag: specTags(NAV_ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf003-${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf003", ownerSession);
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: provisioned.userId, role: 15 }],
        ownerSession
      );
      await serverOnboardSession(provisioned.session);
      const fresh = await serverMe(provisioned.session);
      const cookies = sessionBrowserCookies(provisioned.session);

      await test.step("an empty inbox shows no navigation signal", async () => {
        const unread = await serverNotificationsUnread(seed.workspaceSlug, provisioned.session);
        expect(unread).toEqual({ total: 0, mentions: 0 });
        // The synced entry reads post-load, so null means hidden, not loading.
        const navBadge = await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        expect(navBadge).toBeNull();
        expect(await driver.inboxDotPresent()).toBe(false);
        const fetches = await driver.notificationsEntryFetches(seed.workspaceSlug);
        expect(fetches).toEqual({ list: true, unread: true });
        expect(await driver.notificationsTabBadge("all")).toBeNull();
        expect(await driver.notificationsTabBadge("mentions")).toBeNull();
      });

      const issueName = `Nav badge ${tag}`;
      const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, issueName);
      await test.step("a plain notification shows a dot but no count badge", async () => {
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
          `<p>badge check ${tag}</p>`
        );
        await expect
          .poll(async () => serverNotificationsUnread(seed.workspaceSlug, provisioned.session), {
            timeout: 90_000,
          })
          .toEqual({ total: 1, mentions: 0 });
        // The bug: the unread dot appears but no count badge ever renders.
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await expect.poll(() => driver.inboxDotPresent(), { timeout: 30_000 }).toBe(true);
        expect(await driver.notificationsNavBadge()).toBeNull();
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe("1");
        expect(await driver.notificationsTabBadge("mentions")).toBeNull();
      });

      await test.step("a mention still shows a dot but no marked count", async () => {
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          memberSession,
          serverMentionHtml(provisioned.userId, fresh.displayName, `mention check ${tag}`)
        );
        await expect
          .poll(async () => serverNotificationsUnread(seed.workspaceSlug, provisioned.session), {
            timeout: 90_000,
          })
          .toEqual({ total: 1, mentions: 1 });
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await expect.poll(() => driver.inboxDotPresent(), { timeout: 30_000 }).toBe(true);
        expect(await driver.notificationsNavBadge()).toBeNull();
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        await expect.poll(() => driver.notificationsTabBadge("mentions"), { timeout: 30_000 }).toBe("1");
        await driver.notificationsSelectTab("mentions");
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
            timeout: 30_000,
          })
          .toContain(issueName);
      });
    } finally {
      const memberships = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
      const membership = memberships.find((row) => row.userId === provisioned.userId);
      if (membership !== undefined) {
        await serverCleanupWorkspaceMember(seed.workspaceSlug, membership.membershipId, ownerSession);
      }
    }
  }
);
