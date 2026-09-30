// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-088"], "the four preference commands render and a selection persists to the profile"),
    { tag: specTags(["SHELL-088"]) },
    async ({ driver, seed }) => {
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
