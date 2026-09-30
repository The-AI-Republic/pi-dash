// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — open chord and fresh-state reset (NEWFRONT-127).
// Rows: SHELL-080 (single modified chord opens from anywhere, even while
// typing; no route/slash opener) and SHELL-082 (centered modal mounts,
// backdrop closes, reopening shows a cleared query and the top page).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROOT_PLACEHOLDER = "Type a command or search";

test.describe("command palette open + reset", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-080"], "the open chord summons the palette from anywhere, even while typing"),
    { tag: specTags(["SHELL-080"]) },
    async ({ driver }) => {
      const before = await driver.currentUrlPath();

      await test.step("focus a text field and type", async () => {
        await driver.focusAndTypeTopBarSearch("abc");
      });

      await test.step("the modified chord still opens the modal palette", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        expect(await driver.commandPalettePlaceholder()).toBe(ROOT_PLACEHOLDER);
      });

      await test.step("no navigation happened — the chord alone opened it", async () => {
        expect(await driver.currentUrlPath()).toBe(before);
      });
    }
  );

  test(
    specTitle(["SHELL-082"], "the modal opens centered and resets to a fresh state on close"),
    { tag: specTags(["SHELL-082"]) },
    async ({ driver }) => {
      await test.step("open at the root page", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        expect(await driver.commandPalettePlaceholder()).toBe(ROOT_PLACEHOLDER);
      });

      await test.step("type a query and step into a picker sub-page", async () => {
        await driver.typeInCommandPalette("hello");
        expect(await driver.commandPaletteQueryValue()).toBe("hello");
        await driver.pressInCommandPalette("Escape"); // clears the query first
        expect(await driver.commandPaletteQueryValue()).toBe("");
        await driver.activatePaletteCommand("Open a project");
        // On a sub-page the placeholder changes away from the root prompt.
        await expect.poll(() => driver.commandPalettePlaceholder()).not.toBe(ROOT_PLACEHOLDER);
      });

      await test.step("backdrop click closes the palette", async () => {
        await driver.closeCommandPaletteViaBackdrop();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(false);
      });

      await test.step("reopening shows a cleared query back on the top page", async () => {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        expect(await driver.commandPaletteQueryValue()).toBe("");
        expect(await driver.commandPalettePlaceholder()).toBe(ROOT_PLACEHOLDER);
      });
    }
  );
});
