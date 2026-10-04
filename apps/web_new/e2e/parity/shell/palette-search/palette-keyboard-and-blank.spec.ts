// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — in-palette keyboard flow and the blank-query case
// (NEWFRONT-127). Rows: SHELL-083 (arrows move, enter activates, escape
// clears-then-closes, backspace steps back) and SHELL-085 (blank query
// shows commands only with zero network calls and no results section).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROOT_PLACEHOLDER = "Type a command or search";

test.describe("command palette keyboard + blank query", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-085"], "a blank query lists commands only and fires no search request"),
    { tag: specTags(["SHELL-085"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("command groups render, no server-results heading", async () => {
        await expect.poll(() => driver.paletteGroupHeadings()).not.toEqual([]);
        expect(await driver.paletteSearchResultsHeading()).toBeNull();
      });

      await test.step("idling on a blank query issues zero search requests", async () => {
        const count = await driver.countSearchRequests(async () => {
          await driver.page.waitForTimeout(900);
        });
        expect(count).toBe(0);
      });
    }
  );

  test(
    specTitle(["SHELL-083"], "arrows move, escape clears-then-closes, backspace steps back, enter activates"),
    { tag: specTags(["SHELL-083"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await test.step("arrow keys move the highlighted row", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        const first = await driver.paletteSelectedItemText();
        await driver.pressInCommandPalette("ArrowDown");
        await expect.poll(() => driver.paletteSelectedItemText()).not.toBe(first);
      });

      await test.step("escape clears the query first, then closes on the second press", async () => {
        await driver.typeInCommandPalette("abc");
        expect(await driver.commandPaletteQueryValue()).toBe("abc");
        await driver.pressInCommandPalette("Escape");
        await expect.poll(() => driver.commandPaletteQueryValue()).toBe("");
        expect(await driver.isCommandPaletteOpen()).toBe(true);
        await driver.pressInCommandPalette("Escape");
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(false);
      });

      await test.step("backspace on an empty query steps back out of a sub-page", async () => {
        await driver.pressPaletteOpenChord();
        await driver.activatePaletteCommand("Open a project");
        await expect.poll(() => driver.commandPalettePlaceholder()).not.toBe(ROOT_PLACEHOLDER);
        await driver.pressInCommandPalette("Backspace");
        await expect.poll(() => driver.commandPalettePlaceholder()).toBe(ROOT_PLACEHOLDER);
      });

      await test.step("enter activates the highlighted command (closes on select)", async () => {
        // Filter to a single always-present command so it is the selected row.
        await driver.typeInCommandPalette("Toggle app sidebar");
        await expect.poll(() => driver.paletteHasCommand("Toggle app sidebar")).toBe(true);
        // Presence is not selection: the highlight trails the filter, and
        // Enter activates whatever holds it (a stale row keeps the modal open).
        await expect.poll(() => driver.paletteSelectedItemText()).toContain("Toggle app sidebar");
        await driver.pressInCommandPalette("Enter");
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(false);
      });
    }
  );
});
