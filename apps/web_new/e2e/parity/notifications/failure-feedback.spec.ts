// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-202): failure feedback stays lightweight — a
// failed preference save toasts an error while the server keeps the prior
// value, and a failed list fetch surfaces no inline retry control (a
// failed first load falls through to the empty state). Row: NTF-031.
//
// The toggle-position half of the preference failure is pinned by the
// NTF-024 bug scenario (NEWFRONT-211: the toggle stays on the unsaved
// value); this scenario asserts only the toast and the server-kept value,
// then restores. The refresh-failure path shares the same swallowed-error
// code as the entry failure, so entry plus recovery covers the row.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverEmailPreferences,
  serverNotificationsList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-031"];

const PREF = "comment" as const;

test(
  specTitle(ROWS, "a failed preference save toasts an error and keeps the server value"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    await driver.notificationsOpenEmailPreferences();
    const before = (await serverEmailPreferences(ownerSession))[PREF];

    await driver.notificationsFailEmailPreferenceSaves(500);
    try {
      await driver.notificationsEmailPreferencesToggle(PREF);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/failed to update/i);
      expect((await serverEmailPreferences(ownerSession))[PREF]).toBe(before);
    } finally {
      await driver.notificationsClearEmailPreferenceSaveFailure();
    }

    await test.step("restore the preference to the saved value", async () => {
      await driver.notificationsEmailPreferencesToggle(PREF);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
      expect((await serverEmailPreferences(ownerSession))[PREF]).toBe(before);
      expect((await driver.notificationsEmailPreferences())[PREF]).toBe(before);
    });
  }
);

test(
  specTitle(ROWS, "a failed list fetch shows the empty state with no retry control, then recovers"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `ntf031-${Date.now().toString(36)}`;
    const issueName = `Failure probe ${tag}`;
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    await test.step("fan out a card behind the scenes", async () => {
      const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);
      await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, memberSession, `<p>failure ${tag}</p>`);
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

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("a failed first load falls through to the empty state with no retry", async () => {
      await driver.notificationsFailListFetches(500);
      try {
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        await expect.poll(() => driver.notificationsEmptyText(), { timeout: 30_000 }).not.toBeNull();
        expect(await driver.notificationsRetryControlVisible()).toBe(false);
        // The server holds items throughout: the empty view is the
        // failure fall-through, not a true empty.
        const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
        expect(rows.some((row) => row.issueName === issueName)).toBe(true);
      } finally {
        await driver.notificationsClearListFetchFailure();
      }
    });

    await test.step("clearing the failure lets a refresh recover the list", async () => {
      await driver.notificationsRefresh();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), { timeout: 30_000 })
        .toContain(issueName);
    });
  }
);
