// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-198): the inbox list shows notifications
// newest-first, each card summarizing who acted, what changed, the
// work-item reference and title, and a relative age; unread cards carry a
// marker dot and a tinted background, both cleared once the card is read.
// Rows: NTF-005, NTF-006.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out with sequential member comments (each awaited
// server-side before the next is posted, so the order is deterministic),
// then the assertions key on those titles, so reruns on one seeded stack
// stay green. Read state is flipped through the API; the click-to-read
// control belongs to NTF-007.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationMarkRead,
  serverNotificationsList,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const LIST_ROWS = ["NTF-005"];
const UNREAD_ROWS = ["NTF-006"];

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
  specTitle(LIST_ROWS, "cards list newest-first with actor, change, reference and age"),
  { tag: specTags(LIST_ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf005-${Date.now().toString(36)}`;
    const firstName = `List first ${tag}`;
    const secondName = `List second ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const firstId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, firstName);
    const secondId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, secondName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, firstId, ownerSession, `<p>subscribing ${tag}</p>`);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, secondId, ownerSession, `<p>subscribing ${tag}</p>`);

    let firstRow: NotificationsRow;
    let secondRow: NotificationsRow;
    await test.step("fan out two plain notifications in order", async () => {
      firstRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        firstId,
        `<p>first card ${tag}</p>`,
        ownerSession,
        memberSession
      );
      secondRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        secondId,
        `<p>second card ${tag}</p>`,
        ownerSession,
        memberSession
      );
      expect(secondRow.createdAt >= firstRow.createdAt).toBe(true);
    });

    await test.step("the newer card lists first with its fields", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(secondName);
      const cards = await driver.notificationsCards();
      const titles = cards.map((card) => card.title);
      expect(titles).toContain(firstName);
      expect(titles.indexOf(secondName)).toBeLessThan(titles.indexOf(firstName));
      const second = cards[titles.indexOf(secondName)]!;
      expect(second.actor).toBe(member.displayName);
      expect(second.summary).toContain(member.displayName);
      expect(second.summary.length).toBeGreaterThan(member.displayName.length);
      expect(second.reference).toBe(`${secondRow!.issueIdentifier}-${secondRow!.issueSequenceId}`);
      expect(second.age).not.toBe("");
    });

    await test.step("the server rows match the cards", async () => {
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      const ours = rows.filter((row) => row.entityIdentifier === firstId || row.entityIdentifier === secondId);
      expect(ours).toHaveLength(2);
      for (const row of ours) {
        expect(row.entityName).toBe("issue");
        expect(row.isMentioned).toBe(false);
        expect(row.sender).not.toContain("mentioned");
      }
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      for (const row of ours) {
        expect(titles).toContain(row.issueName);
      }
    });
  }
);

test(
  specTitle(UNREAD_ROWS, "unread cards show a marker dot and tint until read"),
  { tag: specTags(UNREAD_ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf006-${Date.now().toString(36)}`;
    const readName = `Unread read ${tag}`;
    const keptName = `Unread kept ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const readId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, readName);
    const keptId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, keptName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, readId, ownerSession, `<p>subscribing ${tag}</p>`);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, keptId, ownerSession, `<p>subscribing ${tag}</p>`);
    let readRow: NotificationsRow;
    await test.step("fan out two unread notifications", async () => {
      readRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        readId,
        `<p>will read ${tag}</p>`,
        ownerSession,
        memberSession
      );
      await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        keptId,
        `<p>stays unread ${tag}</p>`,
        ownerSession,
        memberSession
      );
      expect(readRow.readAt).toBeNull();
    });

    const cardIndex = async (title: string): Promise<number> =>
      (await driver.notificationsCards()).findIndex((card) => card.title === title);

    await test.step("both fresh cards carry the dot and the same tint", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(() => cardIndex(keptName), { timeout: 30_000 }).not.toBe(-1);
      const cards = await driver.notificationsCards();
      const readCard = cards.find((card) => card.title === readName)!;
      const keptCard = cards.find((card) => card.title === keptName)!;
      expect(readCard.unread).toBe(true);
      expect(keptCard.unread).toBe(true);
      const backgrounds = await driver.notificationsCardBackgrounds();
      expect(backgrounds).toHaveLength(cards.length);
      expect(backgrounds[cards.indexOf(readCard)]).toBe(backgrounds[cards.indexOf(keptCard)]);
    });

    await test.step("reading one card clears its dot and tint", async () => {
      await serverNotificationMarkRead(seed.workspaceSlug, readRow!.id, ownerSession);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === readRow!.id)?.readAt).not.toBeNull();
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).find((card) => card.title === readName), {
          timeout: 30_000,
        })
        .toBeDefined();
      const cards = await driver.notificationsCards();
      const readCard = cards.find((card) => card.title === readName)!;
      const keptCard = cards.find((card) => card.title === keptName)!;
      expect(readCard.unread).toBe(false);
      expect(keptCard.unread).toBe(true);
      const backgrounds = await driver.notificationsCardBackgrounds();
      expect(backgrounds[cards.indexOf(readCard)]).not.toBe(backgrounds[cards.indexOf(keptCard)]);
    });
  }
);
