// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-201): snoozing a notification card to a
// preset delay removes it from the default stream with a confirmation,
// and removing the snooze returns it; a failed snooze write leaves the
// card and the server timestamp untouched. Row: NTF-020.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out with a member comment each, then poll the server
// for the unseen rows, so reruns on one seeded stack stay green.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-020"];

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

async function serverSnoozedTill(
  workspaceSlug: string,
  ownerSession: string,
  notificationId: string
): Promise<string | null> {
  const rows = await serverNotificationsList(workspaceSlug, ownerSession);
  return rows.find((row) => row.id === notificationId)?.snoozedTill ?? null;
}

async function cardIndexByTitle(driver: { notificationsCards(): Promise<{ title: string }[]> }, title: string) {
  const cards = await driver.notificationsCards();
  return cards.findIndex((card) => card.title === title);
}

test(
  specTitle(ROWS, "snooze to a preset removes the card; unsnooze returns it"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf020-${Date.now().toString(36)}`;
    const issueName = `Snooze preset ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
    const row = await fanOutPlain(
      seed.workspaceSlug,
      seed.projectId,
      issueId,
      `<p>snooze me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    await test.step("the picker offers the five preset delays without a remove entry", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
      const index = await cardIndexByTitle(driver, issueName);
      expect(await driver.notificationsSnoozePresets(index)).toEqual([
        "1 day",
        "3 days",
        "5 days",
        "1 week",
        "2 weeks",
      ]);
      expect(await driver.notificationsSnoozeRemovalOffered(index)).toBe(false);
    });

    await test.step("snoozing leaves the default stream with a confirmation", async () => {
      const before = Date.now();
      const index = await cardIndexByTitle(driver, issueName);
      await driver.notificationsSnoozeWithPreset(index, "1 day");
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/snoozed/i);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .not.toContain(issueName);
      const snoozedTill = await serverSnoozedTill(seed.workspaceSlug, ownerSession, row.id);
      expect(snoozedTill).not.toBeNull();
      const resume = new Date(snoozedTill!).getTime();
      expect(resume).toBeGreaterThanOrEqual(before + 24 * 60 * 60 * 1000 - 5 * 60 * 1000);
      expect(resume).toBeLessThanOrEqual(Date.now() + 24 * 60 * 60 * 1000 + 5 * 60 * 1000);
    });

    await test.step("unsnoozing from the snoozed stream returns the card", async () => {
      await driver.notificationsSetSnoozedMode(true);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
      const index = await cardIndexByTitle(driver, issueName);
      expect(await driver.notificationsSnoozeRemovalOffered(index)).toBe(true);
      await driver.notificationsUnsnooze(index);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/unsnoozed/i);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .not.toContain(issueName);
      expect(await serverSnoozedTill(seed.workspaceSlug, ownerSession, row.id)).toBeNull();
      await driver.notificationsSetSnoozedMode(false);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
    });
  }
);

test(
  specTitle(ROWS, "a failed snooze write leaves the card and the timestamp"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf020f-${Date.now().toString(36)}`;
    const issueName = `Snooze failure ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
    const row = await fanOutPlain(
      seed.workspaceSlug,
      seed.projectId,
      issueId,
      `<p>do not snooze me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    await driver.notificationsOpenInbox(seed.workspaceSlug);
    await expect
      .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
        timeout: 30_000,
      })
      .toContain(issueName);

    await test.step("the card stays put while its write fails", async () => {
      await driver.notificationsFailItemWrites(500);
      try {
        const index = await cardIndexByTitle(driver, issueName);
        await driver.notificationsSnoozeWithPreset(index, "3 days");
        // The driver resolves only once the failed PATCH settles, so the
        // optimistic removal has already rolled back when this reads.
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
            timeout: 30_000,
          })
          .toContain(issueName);
        expect(await serverSnoozedTill(seed.workspaceSlug, ownerSession, row.id)).toBeNull();
      } finally {
        await driver.notificationsClearItemWriteFailure();
      }
    });
  }
);
