// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): the mark-all-read control marks every
// notification in scope read at once — listed cards flip read, the current
// tab's counts reset — while showing progress and ignoring repeat presses
// mid-flight. Row: NTF-014.
//
// Isolation runs as a freshly provisioned user: mark-all-read rewrites
// shared inbox state, so the owner's stream is left for the other
// scenarios. Two plain fan-outs plus one mention prove the scope covers
// the stream; the request body proves the filter scope rode along. (The
// navigation count has no badge to reset — NEWFRONT-204 — so only the tab
// count and the server totals are asserted.)
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

const ROWS = ["NTF-014"];

test(
  specTitle(ROWS, "mark-all-read reads the scope at once with progress shown"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf014-${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf014", ownerSession);
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

      const firstName = `Bulk read first ${tag}`;
      const secondName = `Bulk read second ${tag}`;
      const mentionName = `Bulk read mention ${tag}`;
      await test.step("fan two plain plus one mention out to the fresh user", async () => {
        for (const issueName of [firstName, secondName]) {
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
            `<p>bulk check ${tag}</p>`
          );
        }
        const mentionId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, mentionName);
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          mentionId,
          memberSession,
          serverMentionHtml(provisioned.userId, fresh.displayName, `mention check ${tag}`)
        );
        await expect
          .poll(async () => serverNotificationsUnread(seed.workspaceSlug, provisioned.session), {
            timeout: 90_000,
          })
          .toEqual({ total: 2, mentions: 1 });
      });

      await test.step("double-pressing runs one scoped request with progress shown", async () => {
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        // Both entry fetches must land first: badge (unread) and cards
        // (list), so the bulk action cannot race a stale entry response.
        await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe("2");
        await expect.poll(async () => (await driver.notificationsCards()).length, { timeout: 30_000 }).toBe(2);
        const held = await driver.notificationsMarkAllReadHeld(4_000);
        expect(held.progress).toBe(true);
        expect(held.requests).toBe(1);
        expect(held.scopeBody).not.toBeNull();
        const scope = JSON.parse(held.scopeBody!) as Record<string, unknown>;
        expect("snoozed" in scope).toBe(true);
        expect("archived" in scope).toBe(true);
      });

      await test.step("every listed card reads and the counts reset", async () => {
        const cards = await driver.notificationsCards();
        expect(cards.length).toBeGreaterThan(0);
        expect(cards.every((card) => !card.unread)).toBe(true);
        expect(await driver.notificationsTabBadge("all")).toBeNull();
        const unread = await serverNotificationsUnread(seed.workspaceSlug, provisioned.session);
        expect(unread).toEqual({ total: 0, mentions: 0 });
        const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
        expect(rows.length).toBeGreaterThan(0);
        expect(rows.every((row) => row.readAt !== null)).toBe(true);
        const mentions = await serverNotificationsList(seed.workspaceSlug, provisioned.session, {
          mentioned: true,
        });
        expect(mentions.every((row) => row.readAt !== null)).toBe(true);
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
