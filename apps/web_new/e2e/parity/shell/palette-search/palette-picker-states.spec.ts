// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — picker and search empty states (NEWFRONT-127). Row:
// SHELL-093 (a negative row). Picker sub-pages show a plain empty line when
// their filter matches nothing; a server search with no hits shows a
// no-results row ("No results found — Clear search") that resets the query on
// activation; and no history/recents section ever appears while typing. The
// backend-failure-renders-as-empty sub-case needs network-fault injection and
// is deferred to a driver-level fault helper (recorded in the row notes).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const NONSENSE = "zzzqqxnomatch";

function hasRecentsHeading(headings: string[]): boolean {
  return headings.some((h) => /recent|history/i.test(h));
}

test.describe("command palette picker and search empty states", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-093"], "a no-hit server search shows the no-results row and never a recents section"),
    { tag: specTags(["SHELL-093"]) },
    async ({ driver }) => {
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("a blank palette shows no recents/history section", async () => {
        expect(hasRecentsHeading(await driver.paletteGroupHeadings())).toBe(false);
      });

      await test.step("typing a term with no server hits renders the no-results row", async () => {
        await driver.typeInCommandPalette(NONSENSE);
        // The search is debounced (500ms) then hits the server; poll past it.
        await expect.poll(() => driver.paletteHasText("No results found"), { timeout: 15000 }).toBe(true);
        expect(await driver.paletteHasText("Clear search")).toBe(true);
      });

      await test.step("typing never introduces a recents/history section", async () => {
        expect(hasRecentsHeading(await driver.paletteGroupHeadings())).toBe(false);
      });

      await test.step("activating the no-results row clears the query", async () => {
        await driver.activatePaletteCommand("No results found");
        await expect.poll(() => driver.commandPaletteQueryValue()).toBe("");
      });
    }
  );

  test(
    specTitle(["SHELL-093"], "a picker with no matches shows a plain empty line"),
    { tag: specTags(["SHELL-093"]) },
    async ({ driver }) => {
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("open the project picker sub-page", async () => {
        await driver.activatePaletteCommand("Open a project");
        await expect.poll(() => driver.commandPalettePlaceholder()).not.toBe("Type a command or search");
      });

      await test.step("filtering to no match shows the plain empty line", async () => {
        await driver.typeInCommandPalette(NONSENSE);
        await expect.poll(() => driver.paletteHasText("No projects found")).toBe(true);
      });
    }
  );
});
