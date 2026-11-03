// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-202): selection lives only in client state —
// the inbox address never carries selection, filter, or tab parameters,
// and reloading or opening a shared address lands on the unselected view.
// Row: NTF-027.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-027"];

test(
  specTitle(ROWS, "selection leaves no address trace; reload and shared addresses open unselected"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf027-${Date.now().toString(36)}`;
    const issueName = `Link probe ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const addressOf = async (): Promise<URL> => new URL(await driver.notificationsCurrentUrl());
    const expectCleanAddress = async (): Promise<URL> => {
      const url = await addressOf();
      expect(url.search).toBe("");
      expect(url.hash).toBe("");
      return url;
    };

    await test.step("fan out a card behind the scenes", async () => {
      const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, memberSession, `<p>link check ${tag}</p>`);
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

    await test.step("sign in and open the inbox", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.notificationsOpenInbox(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), { timeout: 30_000 })
        .toContain(issueName);
    });

    const serverCount = (await serverNotificationsList(seed.workspaceSlug, ownerSession)).length;

    await test.step("selecting a card opens detail without touching the address", async () => {
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      await driver.notificationsSelectCard(titles.indexOf(issueName));
      expect(await driver.notificationsDetailVariant()).not.toBe("placeholder");
      const url = await expectCleanAddress();
      expect(url.pathname).toContain("/notifications");
    });

    await test.step("tab and filter changes also leave the address clean", async () => {
      await driver.notificationsSelectTab("mentions");
      await expectCleanAddress();
      await driver.notificationsSelectTab("all");
      await expectCleanAddress();
      await driver.notificationsOpenFilterMenu();
      await driver.notificationsToggleFilterOrigin("assigned");
      await expectCleanAddress();
      // The open menu popover covers the clear chip; dismiss it first.
      await driver.notificationsCloseMenus();
      await driver.notificationsClearFilters();
      await expectCleanAddress();
    });

    await test.step("reloading with a card open returns to the unselected view", async () => {
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      await driver.notificationsSelectCard(titles.indexOf(issueName));
      expect(await driver.notificationsDetailVariant()).not.toBe("placeholder");
      await driver.reloadPage();
      // Settle on the mounted tab strip first: the variant reader
      // fallthroughs while the pane is still hydrating.
      await expect.poll(() => driver.notificationsTabNames(), { timeout: 60_000 }).toHaveLength(2);
      await expect.poll(() => driver.notificationsDetailVariant(), { timeout: 30_000 }).toBe("placeholder");
      await expectCleanAddress();
    });

    await test.step("a shared inbox address lands unselected with server state untouched", async () => {
      const pathname = (await addressOf()).pathname;
      await driver.goToPath(pathname);
      await expect.poll(() => driver.notificationsTabNames(), { timeout: 60_000 }).toHaveLength(2);
      await expect.poll(() => driver.notificationsDetailVariant(), { timeout: 30_000 }).toBe("placeholder");
      await expectCleanAddress();
      expect((await serverNotificationsList(seed.workspaceSlug, ownerSession)).length).toBe(serverCount);
    });
  }
);
