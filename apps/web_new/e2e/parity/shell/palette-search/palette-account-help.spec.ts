// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — account, utility and help entries (NEWFRONT-127).
// Row: SHELL-089 (invites, sign-out, sidebar toggle, copy-URL to clipboard
// with a toast, focus-search, the shortcut viewer and external help links
// that leave the app).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("command palette account + help", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-089"], "the palette exposes the account, utility and help entries"),
    { tag: specTags(["SHELL-089"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      for (const label of [
        "Workspace invites",
        "Sign out",
        "Toggle app sidebar",
        "Copy current page URL",
        "Open keyboard shortcuts",
        "Open Pi Dash documentation",
        "Report a bug",
      ]) {
        expect(await driver.paletteHasCommand(label), `palette lists "${label}"`).toBe(true);
      }
    }
  );

  test(
    specTitle(["SHELL-089"], "copy-URL lands on the clipboard with a confirming toast"),
    { tag: specTags(["SHELL-089"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      const url = driver.page.url();

      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
      await driver.activatePaletteCommand("Copy current page URL");

      await test.step("a success toast confirms the copy", async () => {
        await expect.poll(() => driver.hasVisibleText("Current page URL copied to clipboard.")).toBe(true);
      });

      await test.step("the clipboard holds the current page URL", async () => {
        const clip = await driver.page.evaluate(() => navigator.clipboard.readText());
        expect(clip).toBe(url);
      });
    }
  );

  test(
    specTitle(["SHELL-089"], "an external help entry opens the repository in a new tab"),
    { tag: specTags(["SHELL-089"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      const newTab = driver.page.context().waitForEvent("page");
      await driver.activatePaletteCommand("Open Pi Dash documentation");
      const tab = await newTab;
      await tab.waitForLoadState("domcontentloaded").catch(() => undefined);
      expect(tab.url()).toContain("github.com/The-AI-Republic/pi-dash");
    }
  );
});
