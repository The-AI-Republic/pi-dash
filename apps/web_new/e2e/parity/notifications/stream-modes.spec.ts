// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-200): the inbox overflow modes — show unread,
// show archived, show snoozed — where archived and snoozed exclude each
// other and every change reloads the stream from scratch; archived and
// snoozed items stay out of the default stream on their own. Rows: NTF-016,
// NTF-017.
//
// Generation follows the notifications runtime pattern: four issues fanned
// out with sequential member comments, then read/archived/snoozed states set
// through the API, then the assertions key on those titles, so reruns on one
// seeded stack stay green.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationArchive,
  serverNotificationMarkRead,
  serverNotificationSnooze,
  serverNotificationsList,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-016", "NTF-017"];

async function fanOutForIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  bodyHtml: string,
  ownerSession: string,
  memberSession: string
): Promise<NotificationsRow> {
  await serverCreateComment(workspaceSlug, projectId, issueId, memberSession, bodyHtml);
  let found: NotificationsRow | undefined;
  await expect
    .poll(
      async () => {
        const rows = await serverNotificationsList(workspaceSlug, ownerSession);
        found = rows.find((row) => row.entityIdentifier === issueId && !row.isMentioned);
        return found?.id ?? "";
      },
      { timeout: 90_000 }
    )
    .not.toBe("");
  return found!;
}

test(
  specTitle(ROWS, "overflow modes filter the stream with archived/snoozed exclusion"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf016-${Date.now().toString(36)}`;
    const unreadName = `Modes unread ${tag}`;
    const readName = `Modes read ${tag}`;
    const archivedName = `Modes archived ${tag}`;
    const snoozedName = `Modes snoozed ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const unreadId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, unreadName);
    const readId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, readName);
    const archivedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, archivedName);
    const snoozedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, snoozedName);
    let readRow: NotificationsRow;
    let archivedRow: NotificationsRow;
    let snoozedRow: NotificationsRow;
    await test.step("fan out four notifications and shape their states", async () => {
      for (const issueId of [unreadId, readId, archivedId, snoozedId]) {
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          ownerSession,
          `<p>subscribing ${tag}</p>`
        );
      }
      await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        unreadId,
        `<p>stays unread ${tag}</p>`,
        ownerSession,
        memberSession
      );
      readRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        readId,
        `<p>will read ${tag}</p>`,
        ownerSession,
        memberSession
      );
      archivedRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        archivedId,
        `<p>will archive ${tag}</p>`,
        ownerSession,
        memberSession
      );
      snoozedRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        snoozedId,
        `<p>will snooze ${tag}</p>`,
        ownerSession,
        memberSession
      );
      await serverNotificationMarkRead(seed.workspaceSlug, readRow.id, ownerSession);
      await serverNotificationArchive(seed.workspaceSlug, archivedRow.id, ownerSession);
      const resume = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString();
      await serverNotificationSnooze(seed.workspaceSlug, snoozedRow.id, resume, ownerSession);
    });

    const entry = await test.step("the default stream skips archived and snoozed", async () => {
      const query = await driver.notificationsEntryListQuery(seed.workspaceSlug);
      expect(query.read).toBeNull();
      expect(query.archived).toBe("false");
      expect(query.snoozed).toBe("false");
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(unreadName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).toContain(readName);
      expect(titles).not.toContain(archivedName);
      expect(titles).not.toContain(snoozedName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      const ids = rows.map((row) => row.entityIdentifier);
      expect(ids).toContain(unreadId);
      expect(ids).toContain(readId);
      expect(ids).not.toContain(archivedId);
      expect(ids).not.toContain(snoozedId);
      return query;
    });

    await test.step("the overflow menu lists the three modes", async () => {
      await driver.notificationsOpenOverflowMenu();
      const options = await driver.notificationsOverflowOptions();
      expect(options).toHaveLength(3);
      expect(options[0]).toMatch(/unread/i);
      expect(options[1]).toMatch(/archiv/i);
      expect(options[2]).toMatch(/snooz/i);
    });

    await test.step("unread-only shows just unread items", async () => {
      const query = await driver.notificationsToggleMode("unread");
      expect(query.read).toBe("false");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(unreadName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(readName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession, { read: false });
      const ids = rows.map((row) => row.entityIdentifier);
      expect(ids).toContain(unreadId);
      expect(ids).not.toContain(readId);
      await driver.notificationsOpenOverflowMenu();
      const off = await driver.notificationsToggleMode("unread");
      expect(off.read).toBeNull();
      expect(off.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(readName);
    });

    await test.step("archived mode shows only archived items", async () => {
      await driver.notificationsOpenOverflowMenu();
      const query = await driver.notificationsToggleMode("archived");
      expect(query.archived).toBe("true");
      expect(query.snoozed).toBe("false");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(archivedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(unreadName);
      expect(titles).not.toContain(readName);
      expect(titles).not.toContain(snoozedName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession, { archived: true });
      expect(rows.map((row) => row.entityIdentifier)).toContain(archivedId);
    });

    await test.step("snoozed mode replaces archived mode", async () => {
      await driver.notificationsOpenOverflowMenu();
      const query = await driver.notificationsToggleMode("snoozed");
      expect(query.snoozed).toBe("true");
      expect(query.archived).toBe("false");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(snoozedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(archivedName);
      expect(titles).not.toContain(unreadName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession, { snoozed: true });
      expect(rows.map((row) => row.entityIdentifier)).toContain(snoozedId);
    });

    await test.step("archived mode replaces snoozed mode back", async () => {
      await driver.notificationsOpenOverflowMenu();
      const query = await driver.notificationsToggleMode("archived");
      expect(query.archived).toBe("true");
      expect(query.snoozed).toBe("false");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(archivedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(snoozedName);
      await driver.notificationsOpenOverflowMenu();
      const off = await driver.notificationsToggleMode("archived");
      expect(off.archived).toBe("false");
      expect(off.snoozed).toBe("false");
      expect(off.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(unreadName);
      const restored = (await driver.notificationsCards()).map((card) => card.title);
      expect(restored).toContain(readName);
      expect(restored).not.toContain(archivedName);
      expect(restored).not.toContain(snoozedName);
    });
  }
);
