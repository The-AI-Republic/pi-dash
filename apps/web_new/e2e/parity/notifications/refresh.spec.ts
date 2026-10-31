// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): the refresh control reloads the current
// stream — the list reflects the latest server state afterwards — while
// showing progress and ignoring repeat presses mid-flight. Row: NTF-013.
//
// Generation follows the notifications runtime pattern: items fan out after
// the inbox opens (so the list starts stale), keyed on unique titles so
// reruns on one seeded stack stay green. The single-flight branch holds the
// list fetch back and presses twice, then counts the URLs that fired.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-013"];

async function fanOutPlain(
  workspaceSlug: string,
  projectId: string,
  issueName: string,
  tag: string,
  ownerSession: string,
  memberSession: string
): Promise<string> {
  const issueId = await serverCreateIssue(workspaceSlug, projectId, ownerSession, issueName);
  await serverCreateComment(workspaceSlug, projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
  await serverCreateComment(workspaceSlug, projectId, issueId, memberSession, `<p>refresh check ${tag}</p>`);
  await expect
    .poll(
      async () => {
        const rows = await serverNotificationsList(workspaceSlug, ownerSession);
        return rows.some((row) => row.entityIdentifier === issueId && !row.isMentioned);
      },
      { timeout: 90_000 }
    )
    .toBe(true);
  return issueId;
}

test(
  specTitle(ROWS, "refresh reloads the stream once, showing progress mid-flight"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf013-${Date.now().toString(36)}`;
    const firstName = `Refresh first ${tag}`;
    const secondName = `Refresh second ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    await test.step("open the inbox, then fan an item out behind its back", async () => {
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect.poll(async () => (await driver.notificationsCards()).length, { timeout: 30_000 }).toBeGreaterThan(0);
      await fanOutPlain(seed.workspaceSlug, seed.projectId, firstName, tag, ownerSession, memberSession);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(firstName);
    });

    await test.step("refresh pulls the stale list up to the server state", async () => {
      await driver.notificationsRefresh();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(firstName);
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.some((row) => row.issueName === firstName)).toBe(true);
    });

    await test.step("repeat presses mid-flight run one fetch with progress shown", async () => {
      await fanOutPlain(seed.workspaceSlug, seed.projectId, secondName, tag, ownerSession, memberSession);
      const held = await driver.notificationsRefreshHeld(4_000);
      expect(held.spinning).toBe(true);
      expect(held.requests).toHaveLength(1);
      expect(held.requests[0]).toContain("/users/notifications");
      expect(held.requests[0]).toContain("cursor=");
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(secondName);
    });
  }
);
