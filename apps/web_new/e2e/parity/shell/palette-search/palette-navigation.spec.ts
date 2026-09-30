// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — navigation entries and entity pickers (NEWFRONT-127).
// Row: SHELL-087 (direct jumps land on their destination; picker entries
// list entities and navigate on pick).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("command palette navigation", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-087"], "a direct entry jumps straight to its destination"),
    { tag: specTags(["SHELL-087"]) },
    async ({ driver, seed }) => {
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("the palette groups navigation entries", async () => {
        await expect.poll(() => driver.paletteGroupHeadings()).toContain("Navigate");
        expect(await driver.paletteHasCommand("Go to projects list")).toBe(true);
      });

      await test.step("activating 'Go to projects list' lands on the projects route", async () => {
        await driver.activatePaletteCommand("Go to projects list");
        await expect.poll(() => driver.currentUrlPath()).toBe(`/${seed.workspaceSlug}/projects`);
      });
    }
  );

  test(
    specTitle(["SHELL-087"], "a picker entry lists entities and navigates on pick"),
    { tag: specTags(["SHELL-087"]) },
    async ({ driver, seed }) => {
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("open the project picker sub-page", async () => {
        await driver.activatePaletteCommand("Open a project");
        await expect.poll(() => driver.paletteHasCommand(seed.projectName)).toBe(true);
      });

      await test.step("picking the seeded project navigates into its work items", async () => {
        await driver.activatePaletteCommand(seed.projectName);
        await expect.poll(() => driver.currentUrlPath()).toContain(`/projects/${seed.projectId}/issues`);
      });
    }
  );
});
