// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Shortcut reference dialog (NEWFRONT-127). Row: SHELL-094 (its own chord
// opens a dialog listing every bound command grouped by family, with a
// filter that leaves only matching rows).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("keyboard shortcuts dialog", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-094"], "the shortcut chord opens the reference dialog and filtering narrows it"),
    { tag: specTags(["SHELL-094"]) },
    async ({ driver }) => {
      await test.step("the chord opens the dialog", async () => {
        await driver.pressShortcutsDialogChord();
        await expect.poll(() => driver.isShortcutsDialogOpen()).toBe(true);
      });

      await test.step("it lists bound commands", async () => {
        expect(await driver.hasVisibleText("Go to home")).toBe(true);
        expect(await driver.hasVisibleText("Sign out")).toBe(true);
      });

      await test.step("filtering by a command name leaves only its family", async () => {
        await driver.typeShortcutsFilter("home");
        await expect.poll(() => driver.hasVisibleText("Go to home")).toBe(true);
        await expect.poll(() => driver.hasVisibleText("Sign out")).toBe(false);
      });
    }
  );
});
