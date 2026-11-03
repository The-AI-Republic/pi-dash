// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// User-menu scenario (NEWFRONT-125): the menu shows identity with a
// Your-work entry, a settings-page entry and sign-out in sidebar and
// compact top-bar variants.
// Target alignment (NEWFRONT-295 gate): the identity block is plain
// avatar-plus-name-plus-email with no cover art or scrim (main#523,
// main#574); a Your-work entry sits above Settings for admins/members
// and navigates to the current user's profile, closing the menu
// (main#672); Settings navigates to the settings page general tab with
// no dialog (main#573); no Community entry exists and no plan dialog
// opens from the menu (main#572). No oracle: the old app still shows
// the cover, the profile dialog and the community plan dialog.
// Row: SHELL-059.
import { test, expect } from "../../fixtures";
import { WORKSPACE_ROLE_GUEST, ensureWorkspaceMember, ownerSession, workspaceMemberRoles } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-059"];
const GUEST_EMAIL = "parity-usermenu-guest@example.com";
const GUEST_PASSWORD = "Parity-Guest-1";

test(
  specTitle(ROWS, "user menu shows identity, your-work, settings-page entry and sign-out"),
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

    await test.step("the menu shows the identity with your-work, settings and sign-out", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openUserMenu();
      await expect.poll(() => driver.userMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = await driver.userMenuTexts();
      const joined = texts.join(" ");
      expect(joined).toContain(seed.email);
      for (const entry of ["Your work", "Settings", "Sign out"]) {
        expect(joined).toContain(entry);
      }
      expect(joined).not.toContain("Community");
      // Your work sits above Settings in both variants.
      const flat = texts.join("\n");
      expect(flat.indexOf("Your work")).toBeLessThan(flat.indexOf("Settings"));
      await driver.dismissTopmost();
    });

    await test.step("your work navigates to the current user's profile and closes the menu", async () => {
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Your work");
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
        .toContain("/profile/");
      await expect.poll(() => driver.userMenuTexts(), { timeout: 30_000 }).toEqual([]);
    });

    await test.step("settings navigates to the settings page general tab with no dialog", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Settings");
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
        .toBe(
          driver.target === "web_new" ? `/${seed.workspaceSlug}/settings/account/general/` : "/settings/profile/general"
        );
      expect(await driver.isDialogWithTextVisible("Your profile")).toBe(false);
    });

    await test.step("guests see settings and sign-out but no your-work entry", async () => {
      const session = await ownerSession(seed);
      await ensureWorkspaceMember(seed.workspaceSlug, session, GUEST_EMAIL, GUEST_PASSWORD, WORKSPACE_ROLE_GUEST);
      const server = await workspaceMemberRoles(seed.workspaceSlug, session);
      expect(server.find((member) => member.email === GUEST_EMAIL)?.role).toBe(WORKSPACE_ROLE_GUEST);
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(GUEST_EMAIL, GUEST_PASSWORD);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openUserMenu();
      await expect.poll(() => driver.userMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = (await driver.userMenuTexts()).join(" ");
      expect(texts).toContain("Settings");
      expect(texts).toContain("Sign out");
      expect(texts).not.toContain("Your work");
      expect(texts).not.toContain("Community");
      await driver.dismissTopmost();
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
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
      for (const entry of ["Your work", "Settings", "Sign out"]) {
        expect(texts).toContain(entry);
      }
      expect(texts).not.toContain("Community");
      await driver.dismissTopmost();
      // Neither opening nor closing the compact menu disturbs the collapsed
      // sidebar or pins the peek overlay open.
      expect(await driver.isSidebarCollapsed()).toBe(true);
      await expect.poll(() => driver.isSidebarPeekVisible(), { timeout: 15_000 }).toBe(false);
      // Gate note (NEWFRONT-295): the old app stores one string flag;
      // web_new persists a per-panel JSON snapshot under its own key
      // (deliberately unshared during migration).
      if (driver.target === "web_new") {
        const snapshot = await driver.page.evaluate(() => localStorage.getItem("pidash:sidebar-panel:main"));
        expect((JSON.parse(snapshot ?? "{}") as { sidebarCollapsed?: unknown }).sidebarCollapsed).toBe(true);
      } else {
        const stored = await driver.page.evaluate(() => localStorage.getItem("app_sidebar_collapsed"));
        expect(stored).toBe("true");
      }
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
