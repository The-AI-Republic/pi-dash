// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-201): the custom snooze dialog requires both
// a resume date and a time slot, snoozes to the composed moment with a
// confirmation, and hides elapsed same-day slots while offering the full
// half-hour grid for future days. Row: NTF-021.
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

const ROWS = ["NTF-021"];

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

/** Whether the run started late enough that 01:30 AM today is elapsed. */
function elapsedSlotAvailable(now: Date): boolean {
  return now.getHours() >= 2;
}

test(
  specTitle(ROWS, "custom snooze requires a date and a time, then snoozes to that moment"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf021-${Date.now().toString(36)}`;
    const issueName = `Custom snooze ${tag}`;

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
      `<p>custom snooze me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    await driver.notificationsOpenInbox(seed.workspaceSlug);
    await expect
      .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
        timeout: 30_000,
      })
      .toContain(issueName);
    const cards = await driver.notificationsCards();
    const index = cards.findIndex((card) => card.title === issueName);

    await test.step("submitting with no date and no time keeps the dialog open", async () => {
      await driver.notificationsOpenCustomSnooze(index);
      await driver.notificationsCustomSnoozeSubmit();
      expect(await driver.notificationsCustomSnoozeVisible()).toBe(true);
    });

    await test.step("submitting with a date but no time keeps the dialog open", async () => {
      await driver.notificationsCustomSnoozePickDay(2);
      await driver.notificationsCustomSnoozeSubmit();
      expect(await driver.notificationsCustomSnoozeVisible()).toBe(true);
    });

    await test.step("a full date and time snoozes to that moment", async () => {
      await driver.notificationsCustomSnoozePickTime("AM", "10:30");
      await driver.notificationsCustomSnoozeSubmit();
      // The dialog closes only after the snooze write resolves.
      await expect.poll(() => driver.notificationsCustomSnoozeVisible(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/snoozed/i);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .not.toContain(issueName);
      const target = new Date();
      target.setDate(target.getDate() + 2);
      target.setHours(10, 30, 0, 0);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      const snoozedTill = rows.find((entry) => entry.id === row.id)?.snoozedTill;
      expect(snoozedTill).not.toBeNull();
      // Instant comparison: the API omits the millis the client sent.
      expect(new Date(snoozedTill!).getTime()).toBe(target.getTime());
    });
  }
);

test(
  specTitle(ROWS, "future days offer the full half-hour grid"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf021g-${Date.now().toString(36)}`;
    const issueName = `Slot grid ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
    await fanOutPlain(
      seed.workspaceSlug,
      seed.projectId,
      issueId,
      `<p>slot grid me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    await driver.notificationsOpenInbox(seed.workspaceSlug);
    await expect
      .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
        timeout: 30_000,
      })
      .toContain(issueName);
    const cards = await driver.notificationsCards();
    const index = cards.findIndex((card) => card.title === issueName);
    await driver.notificationsOpenCustomSnooze(index);
    await driver.notificationsCustomSnoozePickDay(2);

    const morning = await driver.notificationsCustomSnoozeTimeSlots("AM");
    const evening = await driver.notificationsCustomSnoozeTimeSlots("PM");
    expect(morning).toHaveLength(24);
    expect(evening).toHaveLength(24);
    expect(morning).toContain("10:30");
    expect(evening).toContain("10:30");
  }
);

test(specTitle(ROWS, "an elapsed same-day slot is not offered"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  // 01:30 AM is elapsed on any run past 02:00 and sidesteps the 12
  // AM/PM composition quirk the last scenario pins; inside the first
  // two hours past midnight no unambiguous elapsed slot exists.
  test.skip(!elapsedSlotAvailable(new Date()), "run started before 02:00; no unambiguous elapsed slot");
  const member = requireMentionMember(seed);
  const tag = `ntf021e-${Date.now().toString(36)}`;
  const issueName = `Elapsed slot ${tag}`;

  await test.step("sign in through the UI", async () => {
    await driver.signInWithPassword(seed.email, seed.password);
  });

  const ownerSession = await signInSession(seed.email, seed.password);
  const memberSession = await signInSession(member.email, member.password);
  const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
  await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
  await fanOutPlain(
    seed.workspaceSlug,
    seed.projectId,
    issueId,
    `<p>elapsed slot me ${tag}</p>`,
    ownerSession,
    memberSession
  );

  await driver.notificationsOpenInbox(seed.workspaceSlug);
  await expect
    .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
      timeout: 30_000,
    })
    .toContain(issueName);
  const cards = await driver.notificationsCards();
  const index = cards.findIndex((card) => card.title === issueName);
  await driver.notificationsOpenCustomSnooze(index);
  await driver.notificationsCustomSnoozePickDay(0);

  const offered = await driver.notificationsCustomSnoozeTimeSlots("AM");
  expect(offered).not.toContain("01:30");
  for (const slot of offered) {
    expect(slot).toMatch(/^\d{2}:(00|30)$/);
  }
});

// The 12-hour composition swaps noon and midnight: 12 AM lands on noon
// and 12 PM on midnight. This scenario pins the observed 12 PM outcome;
// the row records the intended noon behavior with the linked issue.
test(
  specTitle(ROWS, "bug: NEWFRONT-213 custom 12 PM composes to midnight instead of noon"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf021b-${Date.now().toString(36)}`;
    const issueName = `Noon quirk ${tag}`;

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
      `<p>noon quirk me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    await driver.notificationsOpenInbox(seed.workspaceSlug);
    await expect
      .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
        timeout: 30_000,
      })
      .toContain(issueName);
    const cards = await driver.notificationsCards();
    const index = cards.findIndex((card) => card.title === issueName);
    await driver.notificationsOpenCustomSnooze(index);
    await driver.notificationsCustomSnoozePickDay(2);
    await driver.notificationsCustomSnoozePickTime("PM", "12:30");
    await driver.notificationsCustomSnoozeSubmit();
    await expect.poll(() => driver.notificationsCustomSnoozeVisible(), { timeout: 30_000 }).toBe(false);

    const buggy = new Date();
    buggy.setDate(buggy.getDate() + 2);
    buggy.setHours(0, 30, 0, 0);
    const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
    const snoozedTill = rows.find((entry) => entry.id === row.id)?.snoozedTill;
    expect(snoozedTill).not.toBeNull();
    // Instant comparison: the API omits the millis the client sent.
    expect(new Date(snoozedTill!).getTime()).toBe(buggy.getTime());
  }
);
