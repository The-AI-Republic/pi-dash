// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shortcut reference dialog (NEWFRONT-127). Row: SHELL-094 (its own chord
// opens a dialog listing every bound command grouped by family, with a
// filter that leaves only matching rows).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("keyboard shortcuts dialog", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-094"], "the shortcut chord opens the reference dialog and filtering narrows it"),
    { tag: specTags(["SHELL-094"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await test.step("the chord opens the dialog", async () => {
        await driver.pressShortcutsDialogChord();
        await expect.poll(() => driver.isShortcutsDialogOpen()).toBe(true);
      });

      await test.step("it lists bound commands", async () => {
        expect(await driver.hasVisibleText("Go to home")).toBe(true);
        // Only shortcut-bound commands are listed ("Sign out" has no binding
        // and is correctly absent); "Copy current page URL" is bound.
        expect(await driver.hasVisibleText("Copy current page URL")).toBe(true);
      });

      await test.step("filtering by a command name leaves only its family", async () => {
        await driver.typeShortcutsFilter("home");
        await expect.poll(() => driver.hasVisibleText("Go to home")).toBe(true);
        await expect.poll(() => driver.hasVisibleText("Copy current page URL")).toBe(false);
      });
    }
  );
});
