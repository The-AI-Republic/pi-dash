// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): triage-queue items embed the triage
// detail view while ordinary work items open in the peek overview; a
// loading indicator covers the project-access lookup, and closing the
// preview clears the selection. Row: NTF-008.
//
// Generation follows the notifications runtime pattern: the triage item
// is submitted through the intake queue API (leaving the seeded issues
// untouched) and the ordinary item through the issues API, each fanned
// out with a sequential member comment keyed on unique titles, so reruns
// on one seeded stack stay green.
import { test, expect } from "../fixtures";
import {
  createIntakeIssue,
  ensureProjectIntake,
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-008"];

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
  specTitle(ROWS, "triage items embed triage, ordinary items peek, close clears selection"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf008-${Date.now().toString(36)}`;
    const triageName = `Triage detail ${tag}`;
    const plainName = `Peek detail ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    await ensureProjectIntake(seed.workspaceSlug, ownerSession, seed.projectId);

    let triageRow: NotificationsRow;
    let plainRow: NotificationsRow;
    await test.step("fan out one triage and one ordinary notification", async () => {
      const triageIssue = await createIntakeIssue(seed.workspaceSlug, seed.projectId, ownerSession, triageName);
      await serverCreateComment(
        seed.workspaceSlug,
        seed.projectId,
        triageIssue.id,
        ownerSession,
        `<p>subscribing ${tag}</p>`
      );
      triageRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        triageIssue.id,
        `<p>triage check ${tag}</p>`,
        ownerSession,
        memberSession
      );
      const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, plainName);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, plainId, ownerSession, `<p>subscribing ${tag}</p>`);
      plainRow = await fanOutPlain(
        seed.workspaceSlug,
        seed.projectId,
        plainId,
        `<p>peek check ${tag}</p>`,
        ownerSession,
        memberSession
      );
      expect(triageRow.isInboxIssue).toBe(true);
      expect(plainRow.isInboxIssue).toBe(false);
    });
    const triageReference = `${triageRow!.issueIdentifier}-${triageRow!.issueSequenceId}`;
    const plainReference = `${plainRow!.issueIdentifier}-${plainRow!.issueSequenceId}`;

    const cardIndex = async (title: string): Promise<number> =>
      (await driver.notificationsCards()).findIndex((card) => card.title === title);

    await test.step("the triage card embeds the triage view behind a spinner", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(() => cardIndex(triageName), { timeout: 30_000 }).not.toBe(-1);
      await expect.poll(() => cardIndex(plainName), { timeout: 30_000 }).not.toBe(-1);
      // First selection on a fresh page, so the access lookup really flies.
      const held = await driver.notificationsSelectCardHeldAccess(await cardIndex(triageName), 4_000);
      expect(held.spinnerShown).toBe(true);
      expect(await driver.notificationsDetailVariant()).toBe("triage");
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(triageReference);
    });

    await test.step("closing the triage preview clears the selection", async () => {
      await driver.notificationsCloseDetail();
      expect(await driver.notificationsDetailVariant()).toBe("placeholder");
    });

    await test.step("the ordinary card opens the peek overview", async () => {
      await driver.notificationsSelectCard(await cardIndex(plainName));
      expect(await driver.notificationsDetailVariant()).toBe("peek");
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(plainReference);
      await expect.poll(() => driver.notificationsDetailText(), { timeout: 30_000 }).toContain(tag);
    });

    await test.step("closing the peek preview clears the selection", async () => {
      await driver.notificationsCloseDetail();
      expect(await driver.notificationsDetailVariant()).toBe("placeholder");
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.find((row) => row.id === triageRow!.id)?.isInboxIssue).toBe(true);
      expect(rows.find((row) => row.id === plainRow!.id)?.isInboxIssue).toBe(false);
    });
  }
);
