// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-125): the help menu links docs, support
// contact and forum plus an in-app shortcut list, product updates and the
// version number.
// Observed on the running old app: the help entry (an icon-only top-bar
// button in this build) opens a menu with documentation, contact,
// keyboard shortcuts, product updates and forum entries plus a version
// footer; external entries leave the app in a new tab while the shortcut
// list and the product-updates dialog open in place. Row: SHELL-060.
import { test, expect } from "../../fixtures";
import { ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-060"];

test(
  specTitle(ROWS, "help menu links help entries and opens dialogs in place"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    await test.step("prepare server session", async () => {
      await ownerSession(seed);
    });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the help menu lists its entries with the version", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openHelpMenu();
      await expect.poll(() => driver.helpMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = (await driver.helpMenuTexts()).join(" ");
      for (const entry of ["Documentation", "Keyboard shortcuts", "What's new?", "Forum"]) {
        expect(texts).toContain(entry);
      }
      expect(texts).toMatch(/\d+\.\d+\.\d+/);
    });

    await test.step("the shortcut list opens in place", async () => {
      const popup = await driver.activateHelpEntry("Keyboard shortcuts");
      expect(popup).toBeNull();
      await expect.poll(() => driver.isDialogWithTextVisible("shortcuts"), { timeout: 30_000 }).toBe(true);
      await driver.dismissTopmost();
      await expect.poll(() => driver.isDialogWithTextVisible("shortcuts"), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the product-updates dialog opens in place", async () => {
      await driver.openHelpMenu();
      const popup = await driver.activateHelpEntry("What's new?");
      expect(popup).toBeNull();
      await expect.poll(() => driver.isDialogWithTextVisible("What's new?"), { timeout: 30_000 }).toBe(true);
      // The updates dialog ignores Escape: its observed close path is an
      // overlay click outside the panel.
      await driver.dismissDialogByOverlayClick();
      await expect.poll(() => driver.isDialogWithTextVisible("What's new?"), { timeout: 30_000 }).toBe(false);
    });

    await test.step("external entries leave the app in a new tab", async () => {
      await driver.openHelpMenu();
      const popup = await driver.activateHelpEntry("Documentation");
      expect(popup).not.toBeNull();
      expect(new URL(popup ?? "").hostname).not.toBe(new URL(driver.page.url()).hostname);
    });
  }
);
