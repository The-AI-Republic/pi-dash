// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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

const ROOT_PLACEHOLDER = "Type a command or search";

// The profile stores the theme nested (theme.theme); callers poll the
// read-back because the save-then-reload round-trip lands after the click
// returns.
async function profileTheme(session: string): Promise<string> {
  const record = await serverUserProfile(session);
  const theme = record["theme"] as { theme?: unknown } | undefined;
  return String(theme?.theme ?? "");
}

test.describe("preferences: appearance, language, timezone", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  // Two tests, not one: each theme switch persists and reloads the shell
  // (~80s per direction once route chunks recompile), and suite18 starved
  // the Dark half after a ~200s suite-launch hook plus the Light switch —
  // API logs show both PATCHes fired 200, the Dark reload simply never got
  // budget. Same split precedent as the 092 guest/archived pair. File order
  // matters: the seed account persists across tests, so the Light test
  // leaves the account light and the Dark test always proves a real
  // transition (running Dark standalone re-saves idempotently instead).
  test(
    specTitle(["SHELL-095"], "the theme picker lists every option and applies Light"),
    { tag: specTags(["SHELL-095"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      await test.step("open the theme sub-page and see every theme option", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change interface theme");
        for (const option of THEME_OPTIONS) {
          await expect.poll(() => driver.paletteHasCommand(option), { message: option }).toBe(true);
        }
      });

      // Selecting a theme persists it and reloads the shell to apply it; the
      // applied theme is the data-theme attribute of the document root (a
      // reload-apply proves the persisted value, since boot reads the
      // profile). Reloads recompile route chunks on demand, so the attribute
      // lands late on a cold dev server.
      await test.step("selecting Light applies the light theme and persists it", async () => {
        await driver.activatePaletteCommand("Light");
        await expect.poll(() => driver.documentTheme(), { timeout: 120_000 }).toBe("light");
        await expect.poll(() => profileTheme(session), { timeout: 60_000 }).toBe("light");
      });
    }
  );

  test(
    specTitle(["SHELL-095"], "selecting Dark applies the dark theme to the document"),
    { tag: specTags(["SHELL-095"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      await test.step("selecting Dark applies the dark theme and persists it", async () => {
        await expect.poll(() => driver.hasVisibleText(seed.projectName), { timeout: 90_000 }).toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change interface theme");
        await driver.activatePaletteCommand("Dark");
        await expect.poll(() => driver.documentTheme(), { timeout: 120_000 }).toBe("dark");
        await expect.poll(() => profileTheme(session), { timeout: 60_000 }).toBe("dark");
      });
    }
  );

  test(
    specTitle(["SHELL-096"], "the OSS build offers English only and selecting it sets the interface language"),
    { tag: specTags(["SHELL-096"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
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
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const before = String((await serverUserAccount(session))["user_timezone"] ?? "");
      // The seed account persists across runs, so pick a zone that differs
      // from whatever is stored — otherwise the change assertion is vacuous.
      // (Europe/London is not in the server list at all; Sydney is the
      // fallback: label "Sydney", value "Australia/Sydney".)
      const target = before.includes("Tokyo") ? "Sydney" : "Tokyo";

      await test.step("open the timezone sub-page and search for a zone", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change timezone");
        // Settle on the sub-page before typing: keystrokes fired mid-transition
        // are lost and the filter never matches.
        await expect.poll(() => driver.commandPalettePlaceholder()).toBe("Change timezone");
        // The sub-page renders a server-driven list that fetches on mount;
        // wait for the target to arrive unfiltered before typing, re-entering
        // when the fetch flakes (a typed query over an unloaded list filters
        // everything out and never recovers).
        let loaded = false;
        for (let attempt = 0; attempt < 3 && !loaded; attempt++) {
          try {
            await expect.poll(() => driver.paletteHasCommand(target), { timeout: 30_000 }).toBe(true);
            loaded = true;
          } catch {
            await driver.pressInCommandPalette("Backspace");
            await expect.poll(() => driver.commandPalettePlaceholder()).toBe(ROOT_PLACEHOLDER);
            await driver.activatePaletteCommand("Change timezone");
            await expect.poll(() => driver.commandPalettePlaceholder()).toBe("Change timezone");
          }
        }
        expect(loaded).toBe(true);
        await driver.typeInCommandPalette(target);
        await expect.poll(() => driver.commandPaletteQueryValue()).toBe(target);
        await expect.poll(() => driver.paletteHasCommand(target), { timeout: 60_000 }).toBe(true);
      });

      await test.step("choosing a zone persists user_timezone and confirms with a toast", async () => {
        await driver.activatePaletteCommand(target);
        // The toast fires on the mutation response and auto-dismisses within
        // seconds, so catch it before the slower server round-trips below.
        await expect
          .poll(() => driver.hasVisibleText("Timezone updated successfully."), { timeout: 30_000 })
          .toBe(true);
        await expect
          .poll(async () => String((await serverUserAccount(session))["user_timezone"] ?? ""))
          .not.toBe(before);
        const after = String((await serverUserAccount(session))["user_timezone"] ?? "");
        expect(after).toContain(target);
      });
    }
  );
});
