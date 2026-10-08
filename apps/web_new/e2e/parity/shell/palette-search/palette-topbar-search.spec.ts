// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search box (NEWFRONT-127). Row: SHELL-081. The top navigation
// always mounts a plain-text search field; focusing it opens an inline
// (non-dialog) cmdk results panel with the same commands plus server hits as
// the modal palette. Escape clears the term and closes; an outside click
// closes; closing resets the term.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const TOPBAR_PLACEHOLDER = "Search commands...";

test.describe("top-bar search box", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-081"], "the top-bar box opens inline results, filters, and resets on close"),
    { tag: specTags(["SHELL-081"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await test.step("the top-bar search box is always visible and starts closed", async () => {
        expect(await driver.topBarSearchPlaceholder()).toBe(TOPBAR_PLACEHOLDER);
        expect(await driver.isTopBarResultsOpen()).toBe(false);
      });

      await test.step("focus opens the inline results panel, not the modal palette", async () => {
        await driver.focusTopBarSearch();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
        expect(await driver.isCommandPaletteOpen()).toBe(false);
        const titles = await driver.topBarResultsCommandTitles();
        expect(titles.length).toBeGreaterThan(0);
      });

      await test.step("typing filters to matching entries", async () => {
        await driver.typeInTopBarSearch("Change interface theme");
        await expect.poll(() => driver.topBarResultsCommandTitles()).toContain("Change interface theme");
      });

      await test.step("Escape clears the term and closes the panel", async () => {
        await driver.pressInTopBarSearch("Escape");
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        expect(await driver.topBarSearchValue()).toBe("");
      });

      await test.step("an outside click closes and resets the term", async () => {
        await driver.focusTopBarSearch();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
        await driver.typeInTopBarSearch("abc");
        await driver.closeTopBarViaOutsideClick();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        expect(await driver.topBarSearchValue()).toBe("");
      });
    }
  );
});
