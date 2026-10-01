// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the suppressed app rail (NEWFRONT-126). In the
// seeded build the rail switch is hard off, so no density control, dock
// entries or pinned settings entry ever mount; the content area keeps its
// full padding instead. Density, docking and the settings entry need the
// matching build where the strip is enabled. Rows: SHELL-063, SHELL-064,
// SHELL-099.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-063", "SHELL-064", "SHELL-099"];

test(
  specTitle(ROWS, "app rail stays suppressed and content keeps its padding"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    for (const open of [
      () => driver.openWorkspaceHome(seed.workspaceSlug),
      () => driver.openProjectsList(seed.workspaceSlug),
      () => driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues"),
    ]) {
      await test.step("no rail strip and no rail settings entry", async () => {
        await open();
        expect(await driver.railPresent()).toBe(false);
        const settingsLinks = await driver.page.getByRole("link", { name: "Settings", exact: true }).count();
        expect(settingsLinks).toBe(0);
        expect(await driver.contentPaddingLeft()).toBeGreaterThan(0);
      });
    }

    await test.step("no density or docking controls are offered anywhere", async () => {
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await driver.page.mouse.click(8, 400, { button: "right" });
      await driver.page.waitForTimeout(1000);
      const portal = await driver.page.locator("#context-menu-portal").textContent();
      expect(portal ?? "").not.toMatch(/Icon only|Dock App Rail|Undock App Rail/);
    });
  }
);
