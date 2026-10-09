// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-201): the profile-settings email page shows
// a loader until the preferences arrive, then per-topic toggles that save
// instantly with feedback — including the completed-only refinement nested
// under state changes. Row: NTF-024.
//
// Each toggle round-trips from its current server value and restores it,
// so reruns on one seeded stack stay green whatever earlier runs flipped.
import { test, expect } from "../fixtures";
import { serverEmailPreferences, signInSession, type EmailNotificationPreferences } from "../helpers/api";
import type { NotificationsEmailPref } from "../drivers/parity-driver";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-024"];

const PREFS: NotificationsEmailPref[] = ["property_change", "state_change", "issue_completed", "comment", "mention"];

test(
  specTitle(ROWS, "email toggles load behind a loader and save instantly per topic"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    await test.step("a loader shows until the preferences arrive", async () => {
      expect(await driver.notificationsEmailPreferencesLoaderShown()).toBe(true);
    });

    await test.step("the toggles match the server state with completed-only nested", async () => {
      await driver.notificationsOpenEmailPreferences();
      const server = await serverEmailPreferences(ownerSession);
      expect(await driver.notificationsEmailPreferences()).toEqual({
        property_change: server.property_change,
        state_change: server.state_change,
        issue_completed: server.issue_completed,
        comment: server.comment,
        mention: server.mention,
      } satisfies EmailNotificationPreferences);
      expect(await driver.notificationsEmailPreferencesCompletedNested()).toBe(true);
    });

    for (const pref of PREFS) {
      await test.step(`toggling ${pref} persists instantly, then restores`, async () => {
        const before = (await serverEmailPreferences(ownerSession))[pref];
        await driver.notificationsEmailPreferencesToggle(pref);
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
        expect((await driver.notificationsEmailPreferences())[pref]).toBe(!before);
        expect((await serverEmailPreferences(ownerSession))[pref]).toBe(!before);
        await driver.notificationsEmailPreferencesToggle(pref);
        expect((await driver.notificationsEmailPreferences())[pref]).toBe(before);
        expect((await serverEmailPreferences(ownerSession))[pref]).toBe(before);
      });
    }
  }
);

// A failed save reports the error and the server keeps the prior value,
// but the toggle stays on the flipped, unsaved value instead of springing
// back. This scenario pins that observed split; the row records the
// intended spring-back with the linked issue.
test(
  specTitle(ROWS, "bug: NEWFRONT-211 a failed email save leaves the toggle on the unsaved value"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    await driver.notificationsOpenEmailPreferences();
    const before = (await serverEmailPreferences(ownerSession)).comment;

    await driver.notificationsFailEmailPreferenceSaves(500);
    try {
      await driver.notificationsEmailPreferencesToggle("comment");
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/failed to update/i);
      // Server kept the prior value...
      expect((await serverEmailPreferences(ownerSession)).comment).toBe(before);
      // ...while the toggle shows the value that never saved.
      expect((await driver.notificationsEmailPreferences()).comment).toBe(!before);
    } finally {
      await driver.notificationsClearEmailPreferenceSaveFailure();
    }

    await test.step("restore the toggle to the saved value", async () => {
      await driver.notificationsEmailPreferencesToggle("comment");
      expect((await serverEmailPreferences(ownerSession)).comment).toBe(before);
      expect((await driver.notificationsEmailPreferences()).comment).toBe(before);
    });
  }
);
