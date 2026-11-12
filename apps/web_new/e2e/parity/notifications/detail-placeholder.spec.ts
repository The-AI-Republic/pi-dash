// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): the detail pane shows a neutral
// placeholder illustration whenever no notification is selected,
// including on first entry before any card is picked. Row: NTF-009.
//
// Generation follows the notifications runtime pattern: one fan-out on a
// unique-titled issue, so the placeholder proves itself against a
// non-empty list and reruns on one seeded stack stay green.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-009"];

test(
  specTitle(ROWS, "the detail pane placeholders with no selection"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf009-${Date.now().toString(36)}`;
    const issueName = `Placeholder probe ${tag}`;

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
        `<p>placeholder check ${tag}</p>`
      );
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

    await test.step("first entry placeholders before any card is picked", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(issueName);
      expect(await driver.notificationsDetailVariant()).toBe("placeholder");
    });

    await test.step("selecting then closing returns to the placeholder", async () => {
      const index = (await driver.notificationsCards()).findIndex((card) => card.title === issueName);
      await driver.notificationsSelectCard(index);
      expect(await driver.notificationsDetailVariant()).toBe("peek");
      await driver.notificationsCloseDetail();
      expect(await driver.notificationsDetailVariant()).toBe("placeholder");
    });
  }
);
