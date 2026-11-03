// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): selecting a card opens its detail and
// marks the card read on first open; reopening an already-read card issues
// no write. Row: NTF-007.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out with sequential member comments, then the assertions
// key on those titles and references, so reruns on one seeded stack stay
// green. Badge assertions compare diffs against the server totals, never
// absolute counts, so the seeded pagination rows and sibling runs stack
// up harmlessly underneath.
import { test, expect } from "../fixtures";
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

const ROWS = ["NTF-007"];

async function fanOutPlain(
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
  specTitle(ROWS, "selecting a card opens its detail and marks it read once"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf007-${Date.now().toString(36)}`;
    const ourName = `Detail select ${tag}`;
    const otherName = `Detail other ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const ourId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, ourName);
    const otherId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, otherName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, ourId, ownerSession, `<p>subscribing ${tag}</p>`);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, otherId, ownerSession, `<p>subscribing ${tag}</p>`);

    let ourRow: NotificationsRow;
    await test.step("fan out two plain notifications", async () => {
      ourRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        ourId,
        `<p>detail check ${tag}</p>`,
        ownerSession,
        memberSession
      );
      await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        otherId,
        `<p>other check ${tag}</p>`,
        ownerSession,
        memberSession
      );
      expect(ourRow.readAt).toBeNull();
    });
    const reference = `${ourRow!.issueIdentifier}-${ourRow!.issueSequenceId}`;

    const cardIndex = async (title: string): Promise<number> =>
      (await driver.notificationsCards()).findIndex((card) => card.title === title);

    await test.step("the first open posts a read and shows the peek detail", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(() => cardIndex(ourName), { timeout: 30_000 }).not.toBe(-1);
      const badgeBefore = await driver.notificationsTabBadge("all");
      const unreadBefore = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(badgeBefore).toBe(String(unreadBefore.total));

      const posted = await driver.notificationsSelectCardPostedRead(await cardIndex(ourName));
      expect(posted).toBe(true);
      expect(await driver.notificationsDetailVariant()).toBe("peek");
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(reference);
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(tag);

      const cards = await driver.notificationsCards();
      expect(cards.find((card) => card.title === ourName)?.unread).toBe(false);
      expect(cards.find((card) => card.title === otherName)?.unread).toBe(true);
      const unreadAfter = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(unreadAfter.total).toBe(unreadBefore.total - 1);
      await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe(String(unreadAfter.total));
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === ourRow!.id)?.readAt).not.toBeNull();
    });

    await test.step("reopening the read card issues no write", async () => {
      await driver.notificationsSelectCardPostedRead(await cardIndex(otherName));
      // Let the other card's own first-open write land everywhere before
      // snapshotting, so a late decrement cannot masquerade as a reselect
      // write below.
      await expect
        .poll(
          async () =>
            (await serverNotificationsList(seed.workspaceSlug, ownerSession)).find(
              (row) => row.entityIdentifier === otherId && !row.isMentioned
            )?.readAt ?? null,
          { timeout: 30_000 }
        )
        .not.toBeNull();
      const unreadBefore = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      await expect
        .poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 })
        .toBe(String(unreadBefore.total));
      const posted = await driver.notificationsSelectCardPostedRead(await cardIndex(ourName));
      expect(posted).toBe(false);
      expect(await driver.notificationsDetailVariant()).toBe("peek");
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(reference);
      expect(await driver.notificationsTabBadge("all")).toBe(String(unreadBefore.total));
      const unreadAfter = await serverNotificationsUnread(seed.workspaceSlug, ownerSession);
      expect(unreadAfter).toEqual(unreadBefore);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === ourRow!.id)?.readAt).not.toBeNull();
    });
  }
);
