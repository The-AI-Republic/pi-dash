// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-202): the inbox answers only to pointer
// controls — plausible keyboard input changes nothing, cards cannot be
// dragged into a new order, and no export/import path exists in the list
// pane or the overflow menu. Row: NTF-029.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-029"];

const PROBE_KEYS = ["j", "k", "r", "e", "a", "ArrowDown", "ArrowUp", "Enter", "?"];

async function fanOutPlain(
  workspaceSlug: string,
  projectId: string,
  issueName: string,
  tag: string,
  ownerSession: string,
  memberSession: string
): Promise<void> {
  const issueId = await serverCreateIssue(workspaceSlug, projectId, ownerSession, issueName);
  await serverCreateComment(workspaceSlug, projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
  await serverCreateComment(workspaceSlug, projectId, issueId, memberSession, `<p>probe ${tag}</p>`);
  await expect
    .poll(
      async () => {
        const rows = await serverNotificationsList(workspaceSlug, ownerSession);
        return rows.some((row) => row.entityIdentifier === issueId && !row.isMentioned);
      },
      { timeout: 90_000 }
    )
    .toBe(true);
}

test(
  specTitle(ROWS, "keys and drag change nothing; no export or import path exists"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf029-${Date.now().toString(36)}`;
    const firstName = `Probe first ${tag}`;
    const secondName = `Probe second ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const titlesOf = async (): Promise<string[]> => (await driver.notificationsCards()).map((card) => card.title);

    await test.step("fan out two cards and open the inbox", async () => {
      await fanOutPlain(seed.workspaceSlug, seed.projectId, firstName, tag, ownerSession, memberSession);
      await fanOutPlain(seed.workspaceSlug, seed.projectId, secondName, tag, ownerSession, memberSession);
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(titlesOf, { timeout: 30_000 }).toContain(firstName);
      await expect.poll(titlesOf, { timeout: 30_000 }).toContain(secondName);
    });

    const serverCount = (await serverNotificationsList(seed.workspaceSlug, ownerSession)).length;

    await test.step("keyboard probes leave selection, tabs, cards, and badges untouched", async () => {
      const before = {
        titles: await titlesOf(),
        tab: await driver.notificationsActiveTab(),
        detail: await driver.notificationsDetailVariant(),
        badge: await driver.notificationsTabBadge("all"),
      };
      for (const key of PROBE_KEYS) {
        await driver.notificationsPressKey(key);
      }
      expect(await titlesOf()).toEqual(before.titles);
      expect(await driver.notificationsActiveTab()).toBe(before.tab);
      expect(await driver.notificationsDetailVariant()).toBe(before.detail);
      expect(await driver.notificationsTabBadge("all")).toBe(before.badge);
    });

    await test.step("dragging a card onto another keeps the order", async () => {
      const before = await titlesOf();
      // The drop may land as a pointer click on the target card; the
      // claim is about order, so only the title sequence is pinned.
      await driver.notificationsDragCard(0, 1);
      expect(await titlesOf()).toEqual(before);
    });

    await test.step("no export or import control exists, and the server is untouched", async () => {
      expect(await driver.notificationsExportImportVisible()).toBe(false);
      await driver.notificationsOpenOverflowMenu();
      const options = await driver.notificationsOverflowOptions();
      expect(options.filter((option) => /export|import|download/i.test(option))).toEqual([]);
      await driver.notificationsCloseMenus();
      expect((await serverNotificationsList(seed.workspaceSlug, ownerSession)).length).toBe(serverCount);
    });
  }
);
