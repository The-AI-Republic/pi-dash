// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-202): the same inbox screens, cards, and
// preferences serve every build, and inbox entry talks only to the known
// notification endpoints — with no native-notification permission request,
// so no desktop shell integration is exercised. Row: NTF-030.
//
// The notification tree carries no edition or desktop conditional and no
// browser Notification API use (verified by reading the old sources, which
// also show no ee-overlay notification overrides); this scenario pins the
// observable half: the standard surface renders, and the entry traffic
// allowlist holds.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-030"];

test(
  specTitle(ROWS, "standard inbox and preferences; entry traffic uses only the known endpoints"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf030-${Date.now().toString(36)}`;
    const issueName = `Edition probe ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    await test.step("fan out a card behind the scenes", async () => {
      const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, memberSession, `<p>edition ${tag}</p>`);
      await expect
        .poll(
          async () => {
            const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
            return rows.some((row) => row.entityIdentifier === issueId && !row.isMentioned);
          },
          { timeout: 90_000 }
        )
        .toBe(true);
    });

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.notificationsArmNotificationRequestSpy();
    });

    await test.step("inbox entry talks only to the list and unread endpoints", async () => {
      const paths = (await driver.notificationsEntryRequestPaths(seed.workspaceSlug)).map((path) =>
        path.replace(/\/+$/, "")
      );
      const base = `/api/workspaces/${seed.workspaceSlug}/users/notifications`;
      const allowed = new Set([base, `${base}/unread`]);
      expect(paths).toContain(base);
      expect(paths).toContain(`${base}/unread`);
      expect(paths.filter((path) => !allowed.has(path))).toEqual([]);
    });

    await test.step("the standard inbox surface renders with server agreement", async () => {
      expect(await driver.notificationsListPaneVisible()).toBe(true);
      expect(await driver.notificationsDetailPaneVisible()).toBe(true);
      expect(await driver.notificationsTabNames()).toHaveLength(2);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles.length).toBeGreaterThan(0);
      expect(titles).toContain(issueName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.some((row) => row.issueName === issueName)).toBe(true);
    });

    await test.step("the standard preference toggles render and no native permission is requested", async () => {
      await driver.notificationsOpenEmailPreferences();
      expect(Object.keys(await driver.notificationsEmailPreferences()).sort()).toEqual([
        "comment",
        "issue_completed",
        "mention",
        "property_change",
        "state_change",
      ]);
      expect(await driver.notificationsNotificationRequestCount()).toBe(0);
    });
  }
);
