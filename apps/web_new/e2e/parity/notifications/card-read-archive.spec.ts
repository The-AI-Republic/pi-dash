// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-200): the per-card hover actions — read/unread
// and archive/unarchive toggles with confirmation toasts, where a failed
// write leaves state unchanged; the actions stay hidden until the card is
// hovered. Rows: NTF-018, NTF-019, NTF-023.
//
// Generation follows the notifications runtime pattern: three issues fanned
// out with sequential member comments, then the assertions key on those
// titles, so reruns on one seeded stack stay green.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  serverNotificationsUnread,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-018", "NTF-019", "NTF-023"];

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

async function awaitNoToast(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toBeNull();
}

test(
  specTitle(ROWS, "card actions toggle read and archive on hover with toasts"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf018-${Date.now().toString(36)}`;
    const readName = `Card read ${tag}`;
    const archiveName = `Card archive ${tag}`;
    const failName = `Card fail ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const readId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, readName);
    const archiveId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, archiveName);
    const failId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, failName);
    let readRow: NotificationsRow;
    let archiveRow: NotificationsRow;
    let failRow: NotificationsRow;
    await test.step("fan out three unread notifications", async () => {
      for (const issueId of [readId, archiveId, failId]) {
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          ownerSession,
          `<p>subscribing ${tag}</p>`
        );
      }
      readRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        readId,
        `<p>read toggle ${tag}</p>`,
        ownerSession,
        memberSession
      );
      archiveRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        archiveId,
        `<p>archive toggle ${tag}</p>`,
        ownerSession,
        memberSession
      );
      failRow = await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        failId,
        `<p>failed toggle ${tag}</p>`,
        ownerSession,
        memberSession
      );
      expect(readRow.readAt).toBeNull();
      expect(archiveRow.archivedAt).toBeNull();
      expect(failRow.readAt).toBeNull();
    });

    const cardIndex = async (title: string): Promise<number> =>
      (await driver.notificationsCards()).findIndex((card) => card.title === title);

    await test.step("actions hide at rest and show on hover", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(() => cardIndex(readName), { timeout: 30_000 }).not.toBe(-1);
      await expect.poll(() => cardIndex(archiveName), { timeout: 30_000 }).not.toBe(-1);
      await expect.poll(() => cardIndex(failName), { timeout: 30_000 }).not.toBe(-1);
      const readIdx = await cardIndex(readName);
      const archiveIdx = await cardIndex(archiveName);
      expect(await driver.notificationsCardActionsVisible(readIdx)).toBe(false);
      expect(await driver.notificationsCardActionsVisible(archiveIdx)).toBe(false);
      await driver.notificationsHoverCard(readIdx);
      expect(await driver.notificationsCardActionsVisible(readIdx)).toBe(true);
      expect(await driver.notificationsCardActionsVisible(archiveIdx)).toBe(false);
    });

    await test.step("read then unread flips state with toasts", async () => {
      await awaitNoToast(driver);
      const before = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      await driver.notificationsToggleCardRead(await cardIndex(readName));
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/marked as read/);
      await expect
        .poll(async () => (await driver.notificationsCards()).find((card) => card.title === readName)?.unread, {
          timeout: 30_000,
        })
        .toBe(false);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === readRow.id)?.readAt).not.toBeNull();
      const after = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(after.total).toBe(before.total - 1);
      await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe(String(after.total));

      await awaitNoToast(driver);
      await driver.notificationsToggleCardRead(await cardIndex(readName));
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/marked as unread/);
      await expect
        .poll(async () => (await driver.notificationsCards()).find((card) => card.title === readName)?.unread, {
          timeout: 30_000,
        })
        .toBe(true);
      const restored = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(restored.find((row) => row.id === readRow.id)?.readAt).toBeNull();
      const back = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(back.total).toBe(before.total);
      await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe(String(back.total));
    });

    await test.step("a failed read leaves state unchanged without a toast", async () => {
      await awaitNoToast(driver);
      await driver.notificationsFailNextCardWrite();
      await driver.notificationsToggleCardRead(await cardIndex(failName));
      expect(await driver.toastText()).toBeNull();
      const cards = await driver.notificationsCards();
      expect(cards.find((card) => card.title === failName)?.unread).toBe(true);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === failRow.id)?.readAt).toBeNull();
      await driver.notificationsToggleCardRead(await cardIndex(failName));
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/marked as read/);
      const reread = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(reread.find((row) => row.id === failRow.id)?.readAt).not.toBeNull();
    });

    await test.step("a failed archive leaves state unchanged without a toast", async () => {
      await awaitNoToast(driver);
      await driver.notificationsFailNextCardWrite();
      await driver.notificationsToggleCardArchive(await cardIndex(archiveName));
      expect(await driver.toastText()).toBeNull();
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).toContain(archiveName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === archiveRow.id)?.archivedAt).toBeNull();
    });

    await test.step("archive then unarchive moves the card with toasts", async () => {
      await awaitNoToast(driver);
      await driver.notificationsToggleCardArchive(await cardIndex(archiveName));
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/marked as archived/);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .not.toContain(archiveName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === archiveRow.id)?.archivedAt).not.toBeNull();
      const archived = await serverNotificationsList(seed.workspaceSlug, ownerSession, { archived: true });
      expect(archived.map((row) => row.id)).toContain(archiveRow.id);

      await driver.notificationsOpenOverflowMenu();
      const query = await driver.notificationsToggleMode("archived");
      expect(query.archived).toBe("true");
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(archiveName);
      await awaitNoToast(driver);
      await driver.notificationsToggleCardArchive(await cardIndex(archiveName));
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/un ?archived/);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .not.toContain(archiveName);
      await driver.notificationsOpenOverflowMenu();
      await driver.notificationsToggleMode("archived");
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(archiveName);
      const restored = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(restored.find((row) => row.id === archiveRow.id)?.archivedAt).toBeNull();
    });
  }
);
