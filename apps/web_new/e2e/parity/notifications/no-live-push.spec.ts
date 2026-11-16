// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-202): no live push — arrivals behind the
// inbox's back stay invisible (cards and badges alike) until the user
// refreshes, switches tabs, or remounts the view. Row: NTF-028.
//
// Each arrival lands server-side first (polled, so the negative is
// meaningful); the settle window follows the ISS-221 realtime-absence
// precedent. Filters share the tab-switch refire path, which NTF-015 and
// NTF-016 pin; this scenario covers refresh, tab switch, and remount.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-028"];

const SETTLE_MS = 3_000;

async function fanOutBehind(
  workspaceSlug: string,
  projectId: string,
  issueName: string,
  tag: string,
  ownerSession: string,
  memberSession: string
): Promise<void> {
  const issueId = await serverCreateIssue(workspaceSlug, projectId, ownerSession, issueName);
  await serverCreateComment(workspaceSlug, projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
  await serverCreateComment(workspaceSlug, projectId, issueId, memberSession, `<p>arrival ${tag}</p>`);
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
  specTitle(ROWS, "arrivals stay invisible until refresh, tab switch, or remount"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf028-${Date.now().toString(36)}`;
    const firstName = `Arrival first ${tag}`;
    const secondName = `Arrival second ${tag}`;
    const thirdName = `Arrival third ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const titlesOf = async (): Promise<string[]> => (await driver.notificationsCards()).map((card) => card.title);

    await test.step("sign in and open the inbox", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(async () => (await driver.notificationsCards()).length, { timeout: 30_000 }).toBeGreaterThan(0);
    });

    await test.step("a refresh picks up what no push delivered", async () => {
      const badgeBefore = await driver.notificationsTabBadge("all");
      await fanOutBehind(seed.workspaceSlug, seed.projectId, firstName, tag, ownerSession, memberSession);
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await titlesOf()).not.toContain(firstName);
      expect(await driver.notificationsTabBadge("all")).toBe(badgeBefore);
      await driver.notificationsRefresh();
      await expect.poll(titlesOf, { timeout: 30_000 }).toContain(firstName);
    });

    await test.step("switching tabs away and back picks up the next arrival", async () => {
      await fanOutBehind(seed.workspaceSlug, seed.projectId, secondName, tag, ownerSession, memberSession);
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await titlesOf()).not.toContain(secondName);
      await driver.notificationsSelectTab("mentions");
      await driver.notificationsSelectTab("all");
      await expect.poll(titlesOf, { timeout: 30_000 }).toContain(secondName);
    });

    await test.step("remounting the view picks up the last arrival", async () => {
      await fanOutBehind(seed.workspaceSlug, seed.projectId, thirdName, tag, ownerSession, memberSession);
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await titlesOf()).not.toContain(thirdName);
      await driver.reloadPage();
      await expect.poll(titlesOf, { timeout: 60_000 }).toContain(thirdName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.some((row) => row.issueName === thirdName)).toBe(true);
    });
  }
);
