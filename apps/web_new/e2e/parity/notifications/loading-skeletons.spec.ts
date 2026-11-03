// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-199): skeleton placeholder rows cover the
// inbox list while its initial fetch flies, then give way to cards — or
// to the empty state when the stream holds nothing. Row: NTF-011.
//
// The populated branch runs as the owner (whose seeded stream is never
// empty); the empty branch runs as a freshly provisioned user. Both hold
// the initial list fetch back so the skeleton stays observable, then
// assert what settles. No row counts are pinned beyond empty/non-empty.
import { test, expect } from "../fixtures";
import {
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverNotificationsList,
  serverNotificationsUnread,
  serverOnboardSession,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-011"];

test(
  specTitle(ROWS, "skeletons cover the initial load, then cards replace them"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    const delayed = await driver.notificationsSkeletonOnDelayedEntry(seed.workspaceSlug, 4_000);
    expect(delayed.skeletonShown).toBe(true);
    expect(delayed.settledCards).toBeGreaterThan(0);

    const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
    expect(rows.length).toBeGreaterThan(0);
  }
);

test(
  specTitle(ROWS, "skeletons cover the initial load, then the empty state replaces them"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf011", ownerSession);
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: provisioned.userId, role: 15 }],
        ownerSession
      );
      await serverOnboardSession(provisioned.session);
      const cookies = sessionBrowserCookies(provisioned.session);

      const unread = await serverNotificationsUnread(seed.workspaceSlug, provisioned.session);
      expect(unread).toEqual({ total: 0, mentions: 0 });

      await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
      const delayed = await driver.notificationsSkeletonOnDelayedEntry(seed.workspaceSlug, 4_000);
      expect(delayed.skeletonShown).toBe(true);
      expect(delayed.settledCards).toBe(0);
      expect(await driver.notificationsEmptyText()).not.toBeNull();

      const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
      expect(rows).toHaveLength(0);
    } finally {
      const memberships = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
      const membership = memberships.find((row) => row.userId === provisioned.userId);
      if (membership !== undefined) {
        await serverCleanupWorkspaceMember(seed.workspaceSlug, membership.membershipId, ownerSession);
      }
    }
  }
);
