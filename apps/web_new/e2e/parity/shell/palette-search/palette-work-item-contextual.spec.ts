// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — work-item contextual group (NEWFRONT-127). Row: SHELL-090.
// When the palette opens on a work-item route (the browse route sets the
// work-item context), a "Work item actions" group appears ahead of the generic
// groups. Its copy actions ("Copy ID", "Copy title", "Copy URL") always show;
// its editing actions ("Change state", "Change priority", "Assign to",
// "Delete", …) show for a project admin or member on a non-archived item — the
// seeded owner is a project admin, so all of them render. Archived items expose
// only the copy actions, which the seed cannot exercise (noted in the row).
import { test, expect } from "../../fixtures";
import { serverFirstWorkItemKey, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("command palette work-item contextual group", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  test(
    specTitle(["SHELL-090"], "opening the palette on a work item surfaces its contextual actions"),
    { tag: specTags(["SHELL-090"]) },
    async ({ driver, seed }) => {
      const session = await signInSession(seed.email, seed.password);
      const { key } = await serverFirstWorkItemKey(seed.workspaceSlug, seed.projectId, session);

      await test.step("open a work item on the browse route", async () => {
        await driver.openBrowseWorkItem(seed.workspaceSlug, key);
        await expect.poll(() => driver.browseShowsWorkItemDetail()).toBe(true);
      });

      await test.step("the palette leads with the work-item actions group", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await expect.poll(() => driver.paletteGroupHeadings()).toContain("Work item actions");
      });

      await test.step("copy actions always render", async () => {
        expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
        expect(await driver.paletteHasCommand("Copy ID")).toBe(true);
      });

      await test.step("editing actions render for the admin owner on a non-archived item", async () => {
        await expect.poll(() => driver.paletteHasCommand("Change state")).toBe(true);
        expect(await driver.paletteHasCommand("Change priority")).toBe(true);
        expect(await driver.paletteHasCommand("Assign to")).toBe(true);
        expect(await driver.paletteHasCommand("Delete")).toBe(true);
      });
    }
  );
});
