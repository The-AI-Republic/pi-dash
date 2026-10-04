// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — picker and search empty states (NEWFRONT-127). Row:
// SHELL-093 (a negative row). A picker whose data is empty shows a plain
// empty line (the seeded project has no labels, so the labels picker shows
// "No labels found"); a server search with no hits shows a no-results row
// ("No results found — Clear search") that resets the query on activation;
// and no history/recents section ever appears while typing. The
// backend-failure-renders-as-empty sub-case needs network-fault injection and
// is deferred to a driver-level fault helper (recorded in the row notes).
import { test, expect } from "../../fixtures";
import { serverFirstWorkItemKey, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const NONSENSE = "zzzqqxnomatch";

function hasRecentsHeading(headings: string[]): boolean {
  return headings.some((h) => /recent|history/i.test(h));
}

test.describe("command palette picker and search empty states", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-093"], "a no-hit server search shows the no-results row and never a recents section"),
    { tag: specTags(["SHELL-093"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
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
    specTitle(["SHELL-093"], "a picker with no data shows a plain empty line"),
    { tag: specTags(["SHELL-093"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const { key, name } = await serverFirstWorkItemKey(seed.workspaceSlug, seed.projectId, session);

      await test.step("open the labels picker on a work item", async () => {
        await driver.openBrowseWorkItem(seed.workspaceSlug, key);
        await expect.poll(() => driver.hasVisibleText(name), { timeout: 120_000 }).toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Add labels");
        await expect.poll(() => driver.commandPalettePlaceholder()).not.toBe("Type a command or search");
      });

      await test.step("the data-empty picker shows its plain empty line", async () => {
        await expect.poll(() => driver.paletteHasText("No labels found"), { timeout: 60_000 }).toBe(true);
      });
    }
  );
});
