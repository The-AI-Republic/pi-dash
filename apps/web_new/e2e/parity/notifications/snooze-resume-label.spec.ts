// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-201): a snoozed card shows its scheduled
// resume date and time where its age would be. Row: NTF-022.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out with a member comment each, then poll the server
// for the unseen rows, so reruns on one seeded stack stay green. The
// snooze itself is a server fixture here — the picker path is NTF-020's.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationSnooze,
  serverNotificationsList,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-022"];

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

/** Resume label the app renders for an instant ("Till Oct 12, 2026, 10:30 AM"). */
function expectedResumeLabel(instant: Date): string {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const month = months[instant.getMonth()];
  const day = String(instant.getDate()).padStart(2, "0");
  const hours = instant.getHours();
  const period = hours >= 12 ? "PM" : "AM";
  const twelve = hours % 12 === 0 ? 12 : hours % 12;
  const time = `${String(twelve).padStart(2, "0")}:${String(instant.getMinutes()).padStart(2, "0")} ${period}`;
  return `Till ${month} ${day}, ${instant.getFullYear()}, ${time}`;
}

test(
  specTitle(ROWS, "snoozed cards show the resume date and time instead of the age"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf022-${Date.now().toString(36)}`;
    const issueName = `Resume label ${tag}`;

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
      `<p>resume label me ${tag}</p>`,
      ownerSession,
      memberSession
    );
    // A second, unsnoozed card proves the list finished loading, so the
    // snoozed card's absence below is a settled read, not an empty page.
    const plainName = `Resume plain ${tag}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, plainName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, plainId, ownerSession, `<p>subscribing ${tag}</p>`);
    await fanOutPlain(
      seed.workspaceSlug,
      seed.projectId,
      plainId,
      `<p>resume plain me ${tag}</p>`,
      ownerSession,
      memberSession
    );

    // A mid-morning resume three days out: unambiguous in any timezone and
    // far from day boundaries, so the rendered wall clock is stable.
    const resume = new Date();
    resume.setDate(resume.getDate() + 3);
    resume.setHours(10, 30, 0, 0);
    await serverNotificationSnooze(seed.workspaceSlug, row.id, ownerSession, resume.toISOString());

    await test.step("the snoozed card leaves the default stream", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(plainName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(issueName);
    });

    await test.step("the snoozed stream shows the resume moment on the card", async () => {
      await driver.notificationsSetSnoozedMode(true);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
      const cards = await driver.notificationsCards();
      const age = cards.find((card) => card.title === issueName)!.age;
      expect(age).toMatch(/^Till \w{3} \d{2}, \d{4}, \d{2}:\d{2} (AM|PM)$/);
      expect(age).toBe(expectedResumeLabel(resume));
    });
  }
);
