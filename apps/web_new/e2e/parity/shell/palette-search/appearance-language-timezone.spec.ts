// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Preferences — appearance, language and timezone (NEWFRONT-127). Rows:
// SHELL-095 (theme options listed; picking a theme applies it to the document
// root), SHELL-096 (interface language; the OSS build offers English only and
// selecting it sets the root lang and persists the profile language), and
// SHELL-097 (timezone picker; a searchable zone list whose selection persists
// to the account and surfaces a confirmation toast). Each palette preference
// command opens a cmdk sub-page whose options are ordinary cmdk items, driven
// by the shared palette actions; the persisted result is read back through the
// public REST API so a redesigned surface cannot pass while storing nothing.
import { test, expect } from "../../fixtures";
import { serverUserAccount, serverUserProfile, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const THEME_OPTIONS = [
  "System preference",
  "Light",
  "Dark",
  "Light high contrast",
  "Dark high contrast",
  "Custom theme",
];

test.describe("preferences: appearance, language, timezone", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-095"], "the theme picker lists every option and applies the chosen theme to the document"),
    { tag: specTags(["SHELL-095"]) },
    async ({ driver }) => {
      await test.step("open the theme sub-page and see every theme option", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change interface theme");
        for (const option of THEME_OPTIONS) {
          await expect.poll(() => driver.paletteHasCommand(option), { message: option }).toBe(true);
        }
      });

      await test.step("selecting Dark applies the dark theme to the document root", async () => {
        // Selecting a theme persists it and reloads the shell to apply it; the
        // dark themes mark the document root with a "dark" class.
        await driver.activatePaletteCommand("Dark");
        await expect.poll(() => driver.htmlClassList()).toContain("dark");
      });
    }
  );

  test(
    specTitle(["SHELL-096"], "the OSS build offers English only and selecting it sets the interface language"),
    { tag: specTags(["SHELL-096"]) },
    async ({ driver, seed }) => {
      const session = await signInSession(seed.email, seed.password);

      await test.step("the language sub-page lists English and no other language", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change interface language");
        await expect.poll(() => driver.paletteHasCommand("English")).toBe(true);
        // The OSS build ships English only; no other locale option renders.
        expect(await driver.paletteHasCommand("Español")).toBe(false);
        expect(await driver.paletteHasCommand("Français")).toBe(false);
      });

      await test.step("selecting English sets the document lang and persists the profile language", async () => {
        await driver.activatePaletteCommand("English");
        await expect.poll(() => driver.documentLang()).toBe("en");
        await expect.poll(async () => String((await serverUserProfile(session))["language"])).toBe("en");
      });
    }
  );

  test(
    specTitle(["SHELL-097"], "the timezone picker is searchable and the selection persists to the account"),
    { tag: specTags(["SHELL-097"]) },
    async ({ driver, seed }) => {
      const session = await signInSession(seed.email, seed.password);
      const before = String((await serverUserAccount(session))["user_timezone"] ?? "");

      await test.step("open the timezone sub-page and search for a zone", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change timezone");
        await driver.typeInCommandPalette("Tokyo");
        await expect.poll(() => driver.paletteHasCommand("Tokyo")).toBe(true);
      });

      await test.step("choosing a zone persists user_timezone and confirms with a toast", async () => {
        await driver.activatePaletteCommand("Tokyo");
        await expect
          .poll(async () => String((await serverUserAccount(session))["user_timezone"] ?? ""))
          .not.toBe(before);
        const after = String((await serverUserAccount(session))["user_timezone"] ?? "");
        expect(after).toContain("Tokyo");
        expect(await driver.hasVisibleText("Timezone updated successfully.")).toBe(true);
      });
    }
  );
});
