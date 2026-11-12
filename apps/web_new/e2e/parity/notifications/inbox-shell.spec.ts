// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-198): the workspace inbox opens as a two-pane
// view — a notification list beside a detail area — keeps a fixed share of
// the width for the list on desktop, and yields the list to the detail
// once an item is selected on narrow viewports. Entry fires both the list
// fetch and the unread-count fetch. Row: NTF-001.
//
// Fixture: the owner creates an issue, subscribes by commenting, then the
// seeded second member comments, which fans a notification out to the
// owner. The fan-out runs as a background task, so the spec polls the
// owner's own inbox until the new row lands; it keys on the issue id (a
// unique-titled issue per run), so reruns on one seeded stack stay green.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  serverNotificationsUnread,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-001"];

test(
  specTitle(ROWS, "inbox opens as a two-pane shell with a fixed list share"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf001-${Date.now().toString(36)}`;
    const issueName = `Inbox shell ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await test.step("fan a notification out to the owner", async () => {
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        issueId,
        memberSession,
        `<p>shell check ${tag}</p>`
      );
      await expect
        .poll(
          async () => {
            const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
            return rows.filter((row) => row.entityIdentifier === issueId && !row.isMentioned);
          },
          { timeout: 90_000 }
        )
        .not.toHaveLength(0);
    });

    await test.step("entry fires the list and unread-count fetches", async () => {
      const fetches = await driver.notificationsEntryFetches(seed.workspaceSlug);
      expect(fetches).toEqual({ list: true, unread: true });
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.length).toBeGreaterThan(0);
      const unread = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(unread.total).toBeGreaterThanOrEqual(0);
      expect(unread.mentions).toBeGreaterThanOrEqual(0);
    });

    await test.step("both panes render on desktop with a fixed list share", async () => {
      await driver.setViewportSize(1440, 900);
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      expect(await driver.notificationsListPaneVisible()).toBe(true);
      expect(await driver.notificationsDetailPaneVisible()).toBe(true);
      const widths = await driver.notificationsPaneWidths();
      expect(widths.list).toBeGreaterThan(0);
      expect(widths.detail).toBeGreaterThan(widths.list);
      const share = widths.list / (widths.list + widths.detail);
      expect(share).toBeGreaterThan(0.15);
      expect(share).toBeLessThan(0.35);
    });

    await test.step("the desktop share stays fixed once an item is selected", async () => {
      const before = await driver.notificationsPaneWidths();
      // Selecting also marks the card read; that side effect belongs to
      // NTF-007 — here only the shell widths are asserted.
      await driver.notificationsSelectCard(0);
      const after = await driver.notificationsPaneWidths();
      expect(after.list).toBeGreaterThan(0);
      expect(after.detail).toBeGreaterThan(0);
      expect(Math.abs(after.list - before.list)).toBeLessThan(2);
    });

    await test.step("narrow viewports yield the list to the detail on selection", async () => {
      await driver.setViewportSize(390, 844);
      // A fresh entry clears the client-side selection, restoring the list.
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      expect(await driver.notificationsListPaneVisible()).toBe(true);
      await driver.notificationsSelectCard(0);
      // The collapse animates; poll to the settled widths.
      await expect.poll(async () => (await driver.notificationsPaneWidths()).list, { timeout: 15_000 }).toBeLessThan(2);
      const widths = await driver.notificationsPaneWidths();
      expect(widths.detail).toBeGreaterThan(0);
    });
  }
);
