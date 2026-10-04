// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — creation entries (NEWFRONT-127). Row: SHELL-086. The
// "Create" group lists creation commands ("New work item", "New project",
// "New workspace", …). Most open a scoped creation modal (a dialog separate
// from the palette), while workspace creation routes to its dedicated page
// instead. This scenario proves the group is present, that a creation command
// opens a non-palette dialog, and that workspace creation navigates away.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("command palette creation entries", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-086"], "a creation command opens a scoped creation dialog"),
    { tag: specTags(["SHELL-086"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("the palette groups creation entries", async () => {
        await expect.poll(() => driver.paletteGroupHeadings()).toContain("Create");
        expect(await driver.paletteHasCommand("New work item")).toBe(true);
        expect(await driver.paletteHasCommand("New project")).toBe(true);
      });

      await test.step("activating 'New work item' closes the palette and opens a creation dialog", async () => {
        await driver.activatePaletteCommand("New work item");
        // The creation command closes the palette (closeOnSelect) and hands
        // over to the scoped create-issue modal, titled "Create new work
        // item". (The modal embeds closed cmdk pickers, so the dialog is
        // identified by its title, not by the absence of cmdk.) The modal
        // chunk compiles on demand on the first activation.
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 60_000 }).toBe(false);
        await expect.poll(() => driver.hasVisibleText("Create new work item"), { timeout: 60_000 }).toBe(true);
      });
    }
  );

  test(
    specTitle(["SHELL-086"], "workspace creation routes to its dedicated page instead of a dialog"),
    { tag: specTags(["SHELL-086"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
      expect(await driver.paletteHasCommand("New workspace")).toBe(true);

      await test.step("activating 'New workspace' navigates to the create-workspace page", async () => {
        await driver.activatePaletteCommand("New workspace");
        // The router normalizes the destination with a trailing slash; the
        // first navigation also compiles the route chunk on demand.
        await expect.poll(() => driver.currentUrlPath(), { timeout: 60_000 }).toMatch(/^\/create-workspace\/?$/);
        // The palette close lags the URL flip by a beat; poll it shut.
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(false);
      });
    }
  );
});
