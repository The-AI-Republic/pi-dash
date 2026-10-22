// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-426): profile-settings shell and legacy
// forwards. Rows PROF-009 (deep-linkable tabs; unknown ids strand the page
// on the loader, bug NEWFRONT-430), PROF-010 (grouped shell navigation) and
// PROF-026 (legacy addresses forward with query parameters preserved,
// except the api-tokens forward which drops them, bug NEWFRONT-431).
//
// Observed behavior notes (this checkout): every settings tab lives at its
// own /settings/profile/<tab> address; the sidebar groups the tabs under a
// personal section and a developer section with a workspace shortcut list
// beneath, always headed by the back arrow plus the settings title; tab
// switches navigate to the tab address and the matching entry reads active.
// The settings dialog variant still exists in this checkout (the user menu
// Settings entry opens it on the general tab): main#573 removes it, so the
// dialog steps below pin current behavior and must be reworked once that
// PR merges. Unknown tab ids render only the loading indicator with no
// recovery (bug NEWFRONT-430, intended fallback to the default tab). The
// legacy forwards land on their matching tabs with the query string carried
// over, defaulting bare addresses to the general tab; only the fixed
// api-tokens forward drops the parameters (bug NEWFRONT-431, intended
// carried like its siblings).
import { test, expect } from "../fixtures";
import { browserSessionCookies, ownerSession, signInSession, type AuthedSession } from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";

/** Authenticated cookie jar for the seed admin, for direct settings navigation. */
async function seedAuthed(email: string, password: string): Promise<AuthedSession> {
  const cookie = await signInSession(email, password);
  const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(cookie)?.[1] ?? "";
  return { email, password, userId: "", cookie, csrfToken, apiBase: parityApiUrl(process.env) };
}

test(
  specTitle(["PROF-010"], "settings shell groups tabs, shortcuts and back navigation with link landings"),
  { tag: specTags(["PROF-010"]) },
  async ({ driver, seed }) => {
    // Complete onboarding plus the product tour first: the welcome modal
    // would otherwise cover the user menu (same as SHELL-059).
    await ownerSession(seed);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openWorkspaceHome(seed.workspaceSlug);
    await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");

    await test.step("the user-menu settings entry still opens the dialog variant", async () => {
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Settings");
      if (driver.target === "web_new") {
        // Target per main#573 (PROF-010): no dialog variant exists; the
        // entry navigates to the settings page general tab instead.
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toContain("/settings/profile/general");
        await expect.poll(() => driver.profileSettingsActiveTab(), { timeout: 30_000 }).toBe("Profile");
      } else {
        // Current behavior only: main#573 removes the dialog and sends
        // this entry to the settings page instead. Rework this step then.
        await expect.poll(() => driver.isDialogWithTextVisible("Your profile"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.profileSettingsActiveTab(), { timeout: 30_000 }).toBe("Profile");
        await driver.dismissTopmost();
      }
    });

    await test.step("the shell shows grouped tabs, the title with back control, and shortcuts", async () => {
      await driver.settingsProfileOpen("general", []);
      expect(await driver.settingsProfileShellVisible()).toBe(true);
      expect(await driver.settingsProfileSidebarTitle()).toBe("Profile settings");
      expect(await driver.settingsProfileBackLabel()).toBe("Back to home");
      expect(await driver.settingsProfileGroupLabels()).toEqual(["Your Profile", "Developer", "Workspace"]);
      expect(await driver.settingsProfileGroupTabs("Your Profile")).toEqual([
        "Profile",
        "Preferences",
        "AI Assistant",
        "Auto Project Management",
        "Notifications",
        "Security",
        "Activity",
      ]);
      expect(await driver.settingsProfileGroupTabs("Developer")).toEqual(["Integrations", "Personal Access Tokens"]);
      const shortcuts = await driver.settingsProfileWorkspaceShortcuts();
      // The workspace entry carries its logo initial ahead of the name.
      expect(shortcuts.join("\n")).toContain(seed.workspaceName);
      expect(shortcuts).toContain("Create workspace");
      expect(shortcuts).toContain("Workspace invites");
      expect(await driver.settingsProfileActiveTab()).toBe("Profile");
    });

    await test.step("tab switches land on the tab address with the entry active", async () => {
      // In-app switches carry a trailing slash; direct visits do not.
      await driver.settingsProfileSelectTab("Security");
      expect(await driver.currentPath()).toBe("/settings/profile/security/");
      expect(await driver.settingsProfileActiveTab()).toBe("Security");
      await driver.settingsProfileSelectTab("Profile");
      expect(await driver.currentPath()).toBe("/settings/profile/general/");
      expect(await driver.settingsProfileActiveTab()).toBe("Profile");
    });
  }
);

test(
  specTitle(["PROF-009"], "bug: NEWFRONT-430 unknown settings tab id strands the page on the loader"),
  { tag: specTags(["PROF-009"]) },
  async ({ driver, seed }) => {
    const admin = await seedAuthed(seed.email, seed.password);

    await test.step("a known tab renders the shell at its own address", async () => {
      await driver.settingsProfileOpen("security", browserSessionCookies(admin));
      expect(await driver.currentPath()).toBe("/settings/profile/security");
      expect(await driver.settingsProfileShellVisible()).toBe(true);
      expect(await driver.settingsProfileLoaderOnly()).toBe(false);
    });

    await test.step("an unknown tab id shows only the loader with no recovery", async () => {
      await driver.settingsProfileOpen("bogus-tab-id", browserSessionCookies(admin));
      if (driver.target === "web_new") {
        // Target for NEWFRONT-430 (PROF-009): the unknown id forwards to
        // the default tab instead of stranding the page on the loader
        // (landing on the slashed in-app form of the address).
        expect(await driver.currentPath()).toBe("/settings/profile/general/");
        expect(await driver.settingsProfileShellVisible()).toBe(true);
        expect(await driver.settingsProfileLoaderOnly()).toBe(false);
        expect(await driver.settingsProfileActiveTab()).toBe("Profile");
      } else {
        expect(await driver.currentPath()).toBe("/settings/profile/bogus-tab-id");
        expect(await driver.settingsProfileShellVisible()).toBe(false);
        expect(await driver.settingsProfileLoaderOnly()).toBe(true);
        // Intended: fall back to the default tab (or a not-found state). The
        // loader never resolves, so waiting changes nothing.
        await new Promise((resolve) => setTimeout(resolve, 5_000));
        expect(await driver.currentPath()).toBe("/settings/profile/bogus-tab-id");
        expect(await driver.settingsProfileLoaderOnly()).toBe(true);
      }
    });
  }
);

test(
  specTitle(["PROF-026"], "legacy settings addresses forward to their profile tabs with parameters kept"),
  { tag: specTags(["PROF-026"]) },
  async ({ driver, seed }) => {
    const admin = await seedAuthed(seed.email, seed.password);
    const cookies = browserSessionCookies(admin);

    await test.step("bare settings lands on the general tab, keeping parameters", async () => {
      expect(await driver.settingsProfileOpenPath("/settings", cookies)).toBe("/settings/profile/general");
      expect(await driver.settingsProfileActiveTab()).toBe("Profile");
      expect(await driver.settingsProfileOpenPath("/settings?keep=1&other=2", cookies)).toBe(
        "/settings/profile/general?keep=1&other=2"
      );
    });

    await test.step("workspace account addresses forward per tab, defaulting to general", async () => {
      expect(
        await driver.settingsProfileOpenPath(`/${seed.workspaceSlug}/settings/account/security?keep=2`, cookies)
      ).toBe("/settings/profile/security?keep=2");
      expect(await driver.settingsProfileActiveTab()).toBe("Security");
      expect(await driver.settingsProfileOpenPath(`/${seed.workspaceSlug}/settings/account/`, cookies)).toBe(
        "/settings/profile/general"
      );
    });

    await test.step("legacy profile addresses forward per tab, defaulting to general", async () => {
      expect(await driver.settingsProfileOpenPath("/profile/preferences?keep=4", cookies)).toBe(
        "/settings/profile/preferences?keep=4"
      );
      expect(await driver.settingsProfileActiveTab()).toBe("Preferences");
      expect(await driver.settingsProfileOpenPath("/profile/", cookies)).toBe("/settings/profile/general");
    });

    await test.step("workspace api-tokens lands on the tokens tab", async () => {
      expect(await driver.settingsProfileOpenPath(`/${seed.workspaceSlug}/settings/api-tokens`, cookies)).toBe(
        "/settings/profile/api-tokens/"
      );
      expect(await driver.settingsProfileActiveTab()).toBe("Personal Access Tokens");
    });

    await test.step("an unknown forwarded tab lands on the loader like a direct visit", async () => {
      const landed = await driver.settingsProfileOpenPath(
        `/${seed.workspaceSlug}/settings/account/bogus-tab-id`,
        cookies
      );
      if (driver.target === "web_new") {
        // Target for NEWFRONT-430 (PROF-026): unknown forwarded tabs
        // default to the general tab with the shell rendered.
        expect(landed).toBe("/settings/profile/general");
        expect(await driver.settingsProfileShellVisible()).toBe(true);
        expect(await driver.settingsProfileLoaderOnly()).toBe(false);
        expect(await driver.settingsProfileActiveTab()).toBe("Profile");
      } else {
        expect(landed).toBe("/settings/profile/bogus-tab-id");
        expect(await driver.settingsProfileLoaderOnly()).toBe(true);
      }
    });
  }
);

test(
  specTitle(["PROF-026"], "bug: NEWFRONT-431 workspace api-tokens forward drops query parameters"),
  { tag: specTags(["PROF-026"]) },
  async ({ driver, seed }) => {
    const admin = await seedAuthed(seed.email, seed.password);

    // Intended: parameters carried over like the other three forwards.
    const landed = await driver.settingsProfileOpenPath(
      `/${seed.workspaceSlug}/settings/api-tokens?keep=3`,
      browserSessionCookies(admin)
    );
    if (driver.target === "web_new") {
      // Target for NEWFRONT-431 (PROF-026): the tokens forward keeps its
      // parameters like its siblings (still landing on the slashed form).
      expect(landed).toBe("/settings/profile/api-tokens/?keep=3");
    } else {
      expect(landed).toBe("/settings/profile/api-tokens/");
    }
  }
);
