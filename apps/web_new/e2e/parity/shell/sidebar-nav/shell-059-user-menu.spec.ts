// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the user menu shows identity with cover
// and avatar, profile settings, community plan dialog and sign-out in
// sidebar and compact top-bar variants.
// Observed on the running old app: the sidebar account card opens a menu
// naming the signed-in identity, a profile-settings entry that opens the
// profile dialog on its general tab, a community entry that opens the
// plan dialog, and a sign-out entry that ends the session back at the
// sign-in screen; collapsing the sidebar swaps in the compact top-bar
// variant with the same entries, and opening it leaves the collapsed and
// peek state undisturbed. Row: SHELL-059.
import { test, expect } from "../../fixtures";
import { ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-059"];

test(
  specTitle(ROWS, "user menu shows identity, settings, community and sign-out"),
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

    await test.step("the menu shows the identity with its entries", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openUserMenu();
      await expect.poll(() => driver.userMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = (await driver.userMenuTexts()).join(" ");
      expect(texts).toContain(seed.email);
      for (const entry of ["Settings", "Community", "Sign out"]) {
        expect(texts).toContain(entry);
      }
      await driver.dismissTopmost();
    });

    await test.step("profile settings open on the general tab", async () => {
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Settings");
      await expect.poll(() => driver.isDialogWithTextVisible("Your profile"), { timeout: 30_000 }).toBe(true);
      // The general tab carries the "Profile" label in the dialog's tab list.
      await expect.poll(() => driver.profileSettingsActiveTab(), { timeout: 30_000 }).toBe("Profile");
      await driver.dismissTopmost();
    });

    await test.step("the community entry opens the plan dialog", async () => {
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Community");
      await expect.poll(() => driver.isDialogWithTextVisible("Your plan"), { timeout: 30_000 }).toBe(true);
      await driver.dismissTopmost();
    });

    await test.step("the compact variant keeps the collapsed state undisturbed", async () => {
      await driver.setSidebarCollapsed(true);
      expect(await driver.isSidebarCollapsed()).toBe(true);
      // Polled: the collapse transition fades the peek out over 300ms.
      await expect.poll(() => driver.isSidebarPeekVisible(), { timeout: 15_000 }).toBe(false);
      await driver.openCompactUserMenu();
      await expect.poll(() => driver.userMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = (await driver.userMenuTexts()).join(" ");
      expect(texts).toContain(seed.email);
      for (const entry of ["Settings", "Community", "Sign out"]) {
        expect(texts).toContain(entry);
      }
      await driver.dismissTopmost();
      // Neither opening nor closing the compact menu disturbs the collapsed
      // sidebar or pins the peek overlay open.
      expect(await driver.isSidebarCollapsed()).toBe(true);
      await expect.poll(() => driver.isSidebarPeekVisible(), { timeout: 15_000 }).toBe(false);
      const stored = await driver.page.evaluate(() => localStorage.getItem("app_sidebar_collapsed"));
      expect(stored).toBe("true");
      await driver.setSidebarCollapsed(false);
      expect(await driver.isSidebarCollapsed()).toBe(false);
    });

    await test.step("signing out lands back at the sign-in screen", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Sign out");
      await expect.poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 }).toBe("/");
      await driver.openEntry();
    });
  }
);
