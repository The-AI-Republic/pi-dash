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
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-087"], "a direct entry jumps straight to its destination"),
    { tag: specTags(["SHELL-087"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("the palette groups navigation entries", async () => {
        await expect.poll(() => driver.paletteGroupHeadings()).toContain("Navigate");
        expect(await driver.paletteHasCommand("Go to projects list")).toBe(true);
      });

      await test.step("activating 'Go to projects list' lands on the projects route", async () => {
        await driver.activatePaletteCommand("Go to projects list");
        // The router normalizes the destination with a trailing slash; the
        // first navigation also compiles the route chunk on demand.
        await expect
          .poll(() => driver.currentUrlPath(), { timeout: 60_000 })
          .toMatch(new RegExp(`^/${seed.workspaceSlug}/projects/?$`));
      });
    }
  );

  test(
    specTitle(["SHELL-087"], "a picker entry lists entities and navigates on pick"),
    { tag: specTags(["SHELL-087"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      await test.step("the 'op' sequence opens the palette onto the project picker", async () => {
        // Two-key sequences fire globally outside inputs (typing them into
        // the palette input only filters), so the sequence runs before the
        // palette is ever opened. "op" also sidesteps the "Open a project" /
        // "Open a project setting" text collision a click would hit.
        await driver.pressKey("o");
        await driver.pressKey("p");
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await expect.poll(() => driver.commandPalettePlaceholder()).not.toBe("Type a command or search");
        await expect.poll(() => driver.paletteHasCommand(seed.projectName), { timeout: 60_000 }).toBe(true);
      });

      await test.step("picking the seeded project navigates into its work items", async () => {
        await driver.activatePaletteCommand(seed.projectName);
        await expect
          .poll(() => driver.currentUrlPath(), { timeout: 60_000 })
          .toContain(`/projects/${seed.projectId}/issues`);
        // Settle on landing content, not just the URL: the pick may resolve
        // onto the already-mounted route, and the next step fires the chord.
        await expect.poll(() => driver.hasVisibleText(seed.projectName), { timeout: 60_000 }).toBe(true);
      });

      await test.step("the palette exposes project navigation", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        // Data-backed entries resolve after static ones; poll each presence.
        await expect.poll(() => driver.paletteHasCommand("Go to projects list")).toBe(true);
        await expect.poll(() => driver.paletteHasCommand("Open a project")).toBe(true);
      });
    }
  );
});
