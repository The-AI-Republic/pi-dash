// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command palette — preference entries (NEWFRONT-127). Row: SHELL-088. The
// palette exposes four preference commands ("Change interface theme",
// "Change timezone", "Change first day of week", "Change interface
// language"), each opening a cmdk sub-page. This scenario proves all four
// render in the Preferences group and that a preference selection persists
// to the server: choosing a first-day-of-week writes start_of_the_week to
// the user's profile. Theme, language and timezone persistence are proven in
// appearance-language-timezone.spec.ts (rows SHELL-095/096/097).
import { test, expect } from "../../fixtures";
import { serverUserProfile, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const PREFERENCE_COMMANDS = [
  "Change interface theme",
  "Change timezone",
  "Change first day of week",
  "Change interface language",
];

test.describe("command palette preference entries", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-088"], "the four preference commands render and a selection persists to the profile"),
    { tag: specTags(["SHELL-088"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);

      await test.step("all four preference commands are listed in the palette", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        for (const title of PREFERENCE_COMMANDS) {
          expect(await driver.paletteHasCommand(title), `expected the "${title}" command`).toBe(true);
        }
      });

      await test.step("choosing a first day of week opens its sub-page listing the days", async () => {
        await driver.activatePaletteCommand("Change first day of week");
        // The sub-page lists the selectable days as ordinary cmdk items.
        await expect.poll(() => driver.paletteHasCommand("Monday")).toBe(true);
        expect(await driver.paletteHasCommand("Sunday")).toBe(true);
      });

      await test.step("selecting Monday writes start_of_the_week to the server profile", async () => {
        await driver.activatePaletteCommand("Monday");
        // The old app stores the first day of week as a 0-based index with
        // Sunday = 0, so Monday persists as 1.
        await expect.poll(async () => Number((await serverUserProfile(session))["start_of_the_week"])).toBe(1);
      });
    }
  );
});
