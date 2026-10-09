// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): each stream tab shows its own empty
// illustration while it holds nothing, and the illustration clears as soon
// as items arrive. Row: NTF-012.
//
// The empty branches run as a freshly provisioned user whose inbox starts
// empty; the variants are told apart by their text (which differs per tab)
// without pinning the wording. One plain and one mention fan-out then prove
// each tab clears, keyed on unique titles so reruns stay green.
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
  serverOnboardSession,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-012"];

test(
  specTitle(ROWS, "each tab empties with its own illustration until items arrive"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf012-${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf012", ownerSession);
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

      await test.step("both tabs empty with their own illustration", async () => {
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        expect(await driver.notificationsActiveTab()).toBe("all");
        // The empty state replaces the skeleton only once the entry fetch
        // lands; poll past the loading branch.
        await expect.poll(() => driver.notificationsEmptyText(), { timeout: 30_000 }).not.toBeNull();
        const allText = await driver.notificationsEmptyText();
        expect(allText!).not.toBe("");
        await driver.notificationsSelectTab("mentions");
        await expect.poll(() => driver.notificationsEmptyText(), { timeout: 30_000 }).not.toBeNull();
        const mentionsText = await driver.notificationsEmptyText();
        expect(mentionsText!).not.toBe("");
        expect(mentionsText).not.toBe(allText);
        const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
        expect(rows).toHaveLength(0);
      });

      const plainName = `Empty plain ${tag}`;
      const mentionName = `Empty mention ${tag}`;
      await test.step("fan one plain and one mention out to the fresh user", async () => {
        const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, plainName);
        const mentionId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, mentionName);
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          plainId,
          provisioned.session,
          `<p>subscribing ${tag}</p>`
        );
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          plainId,
          memberSession,
          `<p>empty check ${tag}</p>`
        );
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          mentionId,
          memberSession,
          serverMentionHtml(provisioned.userId, fresh.displayName, `mention check ${tag}`)
        );
        await expect
          .poll(async () => serverNotificationsList(seed.workspaceSlug, provisioned.session), {
            timeout: 90_000,
          })
          .toHaveLength(1);
        await expect
          .poll(async () => serverNotificationsList(seed.workspaceSlug, provisioned.session, { mentioned: true }), {
            timeout: 90_000,
          })
          .toHaveLength(1);
      });

      await test.step("each tab clears once its items arrive", async () => {
        await driver.notificationsSelectTab("all");
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
            timeout: 30_000,
          })
          .toContain(plainName);
        expect(await driver.notificationsEmptyText()).toBeNull();
        await driver.notificationsSelectTab("mentions");
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
            timeout: 30_000,
          })
          .toContain(mentionName);
        expect(await driver.notificationsEmptyText()).toBeNull();
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
