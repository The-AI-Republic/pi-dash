// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-426, reworked by NEWFRONT-1030 for main#670
// and main#675): the settings navigator, the account tabs, the workspace
// section, and the legacy forwards. Rows PROF-009 (deep-linkable tabs;
// unknown ids strand the page on the loader, bug NEWFRONT-430), PROF-010
// (grouped navigator, workspace section, no dialog variant) and PROF-026
// (legacy addresses forward with query parameters preserved, except the
// api-tokens forward which drops them, bug NEWFRONT-431).
//
// Observed behavior notes (the oracle checkout predates main#670/#675): the
// settings page is a standalone full-window frame at /settings/profile/<tab>
// with no app rail or top bar around it; the navigator groups the tabs under
// a personal section and a developer section, with a workspace list beneath
// in which every workspace is a plain link to its home; tab switches land on
// the tab address and the matching entry reads active; the back control
// closes the frame rather than navigating. Unknown tab ids render only the
// loading indicator with no recovery (bug NEWFRONT-430, intended fallback to
// the default tab). The legacy forwards land on their matching tabs with the
// query string carried over, defaulting bare addresses to the general tab;
// only the fixed api-tokens forward drops the parameters (bug NEWFRONT-431,
// intended carried like its siblings). The settings dialog variant is gone
// per main#573, so the user-menu Settings entry navigates to the page.
//
// Target (web_new): account settings live inside the workspace shell at
// /<ws>/settings/account/<tab>; the navigator is the shell's middle panel
// with the app rail and top bar around it; the back control returns to the
// workspace home; each workspace row expands in place to that workspace's
// role-filtered settings pages, the viewed one open by default; only the
// viewed workspace's entries highlight, and none while an account tab is open.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  deleteWorkspace,
  ensureWorkspaceMember,
  ownerSession,
  serverCreateWorkspace,
  signInSession,
  type AuthedSession,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";

/** Authenticated cookie jar for the seed admin, for direct settings navigation. */
async function seedAuthed(email: string, password: string): Promise<AuthedSession> {
  const cookie = await signInSession(email, password);
  const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(cookie)?.[1] ?? "";
  return { email, password, userId: "", cookie, csrfToken, apiBase: parityApiUrl(process.env) };
}

/** Where the settings page of a tab lives on each target. */
function accountTabPath(target: "web_new" | "web"): (slug: string, tab: string) => string {
  return target === "web_new"
    ? (slug, tab) => `/${slug}/settings/account/${tab}/`
    : (_slug, tab) => `/settings/profile/${tab}/`;
}

test(
  specTitle(["PROF-010"], "settings navigator groups tabs, shortcuts and back navigation with link landings"),
  { tag: specTags(["PROF-010"]) },
  async ({ driver, seed }) => {
    const tabPath = accountTabPath(driver.target);
    // Complete onboarding plus the product tour first: the welcome modal
    // would otherwise cover the user menu (same as SHELL-059).
    await ownerSession(seed);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openWorkspaceHome(seed.workspaceSlug);
    await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");

    await test.step("the user-menu settings entry navigates to the settings general tab with no dialog", async () => {
      await driver.openUserMenu();
      await driver.activateUserMenuItem("Settings");
      await expect
        .poll(() => driver.currentPath(), { timeout: 30_000 })
        .toContain(tabPath(seed.workspaceSlug, "general").replace(/\/$/, ""));
      expect(await driver.isDialogWithTextVisible("Your profile")).toBe(false);
      await expect.poll(() => driver.profileSettingsActiveTab(), { timeout: 30_000 }).toBe("Profile");
    });

    await test.step("the settings page rides inside the normal app shell", async () => {
      if (driver.target === "web_new") {
        expect(await driver.settingsProfileInsideAppShell()).toBe(true);
        expect(await driver.settingsProfileMobileMenuPresent()).toBe(true);
      } else {
        // Current behavior: the old settings page is its own full-window frame.
        expect(await driver.settingsProfileInsideAppShell()).toBe(false);
      }
    });

    await test.step("the navigator shows grouped tabs, the title with back control, and shortcuts", async () => {
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

    await test.step("the back control returns to the current workspace's home", async () => {
      if (driver.target === "web_new") {
        expect(await driver.settingsProfileBackHref()).toBe(`/${seed.workspaceSlug}`);
      } else {
        // The old frame closes instead of navigating; recorded as a gap.
        expect(await driver.settingsProfileBackHref()).toBeNull();
      }
    });

    await test.step("tab switches land on the tab address with the entry active", async () => {
      await driver.settingsProfileSelectTab("Security");
      expect(await driver.currentPath()).toBe(tabPath(seed.workspaceSlug, "security"));
      expect(await driver.settingsProfileActiveTab()).toBe("Security");
      await driver.settingsProfileSelectTab("Profile");
      expect(await driver.currentPath()).toBe(tabPath(seed.workspaceSlug, "general"));
      expect(await driver.settingsProfileActiveTab()).toBe("Profile");
    });
  }
);

test(
  specTitle(["PROF-010"], "workspace section expands each workspace's settings pages in place"),
  { tag: specTags(["PROF-010"]) },
  async ({ driver, seed }) => {
    const owner = await ownerSession(seed);
    const stamp = Date.now().toString(36);
    const slug = `parity-670-${stamp}`;
    const name = `Parity 670 ${stamp}`;
    await serverCreateWorkspace(owner, name, slug);
    try {
      const ownerCookies = browserSessionCookies(await signInSession(seed.email, seed.password));
      await driver.settingsProfileOpen("general", ownerCookies);

      await test.step("every workspace row starts with the viewed one open and the rest closed", async () => {
        const rows = await driver.settingsProfileWorkspaceRows();
        const own = rows.find((row) => row.label === seed.workspaceName);
        expect(own).toBeDefined();
        expect(own?.expandable).toBe(true);
        expect(own?.expanded).toBe(true);
        if (driver.target === "web_new") {
          const other = rows.find((row) => row.label === name);
          expect(other).toBeDefined();
          // The other rows exist but stay closed until asked.
          expect(rows.filter((row) => row.label !== seed.workspaceName).every((row) => !row.expanded)).toBe(true);
        } else {
          // Current behavior: no row expands; each links to its home.
          expect(rows.every((row) => !row.expandable)).toBe(true);
        }
      });

      if (driver.target === "web_new") {
        await test.step("the viewed workspace's entries are its role-filtered settings pages", async () => {
          expect(await driver.settingsProfileWorkspaceEntries(seed.workspaceName)).toEqual([
            "General",
            "Members",
            "Billing & Plans",
            "Exports",
            "Webhooks",
            "Integrations (github)",
          ]);
        });

        await test.step("an entry in another workspace opens that workspace's settings", async () => {
          await driver.settingsProfileToggleWorkspaceRow(name);
          const rows = await driver.settingsProfileWorkspaceRows();
          expect(rows.find((row) => row.label === name)?.expanded).toBe(true);
          expect(await driver.settingsProfileWorkspaceEntries(name)).toContain("General");
          await driver.settingsProfileOpenWorkspaceEntry(name, "General");
          expect(await driver.currentPath()).toBe(`/${slug}/settings/`);
          expect(await driver.wsSettingsSidebarVisible()).toBe(true);
        });

        await test.step("while an account tab is open, nothing in the workspace section is lit", async () => {
          await driver.settingsProfileOpen("general", ownerCookies);
          expect(await driver.settingsProfileCurrentWorkspaceEntries()).toEqual([]);
        });
      }
    } finally {
      await deleteWorkspace(owner, slug).catch(() => undefined);
    }
  }
);

test(
  specTitle(["PROF-010"], "a workspace where the reader is a guest is a plain row with no expander"),
  { tag: specTags(["PROF-010"]) },
  async ({ driver, seed }) => {
    const owner = await ownerSession(seed);
    const stamp = Date.now().toString(36);
    const email = `parity-675-${stamp}@example.com`;
    const password = `parity-675-pass-${stamp}`;
    const guestSlug = `parity-675-${stamp}`;
    const guestName = `Parity 675 ${stamp}`;
    await serverCreateWorkspace(owner, guestName, guestSlug);
    await ensureWorkspaceMember(guestSlug, owner, email, password, 5);
    try {
      const member = await signInSession(email, password);
      await driver.settingsProfileOpen("general", browserSessionCookies(member));
      if (driver.target === "web_new") {
        const rows = await driver.settingsProfileWorkspaceRows();
        const guestRow = rows.find((row) => row.label === guestName);
        expect(guestRow).toEqual({ label: guestName, expandable: false, expanded: false });
        // The plain row keeps the way home it had before the nesting.
        expect(await driver.settingsProfileWorkspaceRowHref(guestName)).toBe(`/${guestSlug}/`);
        expect(await driver.settingsProfileWorkspaceEntries(guestName)).toEqual([]);
      } else {
        // Current behavior: every row is such a plain way home already.
        expect(await driver.settingsProfileWorkspaceRowHref(guestName)).toBe(`/${guestSlug}/`);
      }
    } finally {
      await deleteWorkspace(owner, guestSlug).catch(() => undefined);
    }
  }
);

test(
  specTitle(["PROF-009"], "bug: NEWFRONT-430 unknown settings tab id strands the page on the loader"),
  { tag: specTags(["PROF-009"]) },
  async ({ driver, seed }) => {
    const admin = await seedAuthed(seed.email, seed.password);
    const tabPath = accountTabPath(driver.target);

    await test.step("a known tab renders the navigator at its own address", async () => {
      await driver.settingsProfileOpen("security", browserSessionCookies(admin));
      expect(await driver.currentPath()).toBe(tabPath(seed.workspaceSlug, "security").replace(/\/$/, ""));
      expect(await driver.settingsProfileShellVisible()).toBe(true);
      expect(await driver.settingsProfileLoaderOnly()).toBe(false);
    });

    await test.step("an unknown tab id shows only the loader with no recovery", async () => {
      await driver.settingsProfileOpen("bogus-tab-id", browserSessionCookies(admin));
      if (driver.target === "web_new") {
        // Target for NEWFRONT-430 (PROF-009): the unknown id forwards to
        // the default tab instead of stranding the page on the loader
        // (landing on the slashed in-app form of the address).
        expect(await driver.currentPath()).toBe(tabPath(seed.workspaceSlug, "general"));
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
  specTitle(["PROF-026"], "legacy settings addresses forward to their account tabs with parameters kept"),
  { tag: specTags(["PROF-026"]) },
  async ({ driver, seed }) => {
    const admin = await seedAuthed(seed.email, seed.password);
    const cookies = browserSessionCookies(admin);
    const ws = seed.workspaceSlug;
    const account = (tab: string) => accountTabPath(driver.target)(ws, tab);

    await test.step("bare settings lands on the general tab, keeping parameters", async () => {
      expect(await driver.settingsProfileOpenPath("/settings", cookies)).toBe(account("general").replace(/\/$/, ""));
      expect(await driver.settingsProfileActiveTab()).toBe("Profile");
      expect(await driver.settingsProfileOpenPath("/settings?keep=1&other=2", cookies)).toBe(
        `${account("general").replace(/\/$/, "")}?keep=1&other=2`
      );
    });

    await test.step("the legacy profile family forwards per tab, defaulting to general", async () => {
      expect(await driver.settingsProfileOpenPath("/settings/profile/security?keep=2", cookies)).toBe(
        `${account("security").replace(/\/$/, "")}?keep=2`
      );
      expect(await driver.settingsProfileActiveTab()).toBe("Security");
      expect(await driver.settingsProfileOpenPath("/settings/profile/", cookies)).toBe(
        account("general").replace(/\/$/, "")
      );
    });

    await test.step("workspace account addresses forward per tab, defaulting to general", async () => {
      if (driver.target === "web_new") {
        // Target (main#670): the workspace address is the real one, so the
        // tab survives with the trailing slash the app's own links use.
        expect(await driver.settingsProfileOpenPath(`/${ws}/settings/account/security?keep=2`, cookies)).toBe(
          `${account("security")}?keep=2`
        );
        expect(await driver.settingsProfileActiveTab()).toBe("Security");
        expect(await driver.settingsProfileOpenPath(`/${ws}/settings/account/`, cookies)).toBe(account("general"));
      } else {
        expect(await driver.settingsProfileOpenPath(`/${ws}/settings/account/security?keep=2`, cookies)).toBe(
          "/settings/profile/security?keep=2"
        );
        expect(await driver.settingsProfileActiveTab()).toBe("Security");
        expect(await driver.settingsProfileOpenPath(`/${ws}/settings/account/`, cookies)).toBe(
          "/settings/profile/general"
        );
      }
    });

    await test.step("older profile addresses forward per tab, defaulting to general", async () => {
      expect(await driver.settingsProfileOpenPath("/profile/preferences?keep=4", cookies)).toBe(
        `${account("preferences").replace(/\/$/, "")}?keep=4`
      );
      expect(await driver.settingsProfileActiveTab()).toBe("Preferences");
      expect(await driver.settingsProfileOpenPath("/profile/", cookies)).toBe(account("general").replace(/\/$/, ""));
    });

    await test.step("workspace api-tokens lands on the tokens tab", async () => {
      expect(await driver.settingsProfileOpenPath(`/${ws}/settings/api-tokens`, cookies)).toBe(
        driver.target === "web_new" ? account("api-tokens") : "/settings/profile/api-tokens/"
      );
      expect(await driver.settingsProfileActiveTab()).toBe("Personal Access Tokens");
    });

    await test.step("an unknown forwarded tab lands on the loader like a direct visit", async () => {
      const landed = await driver.settingsProfileOpenPath(`/${ws}/settings/account/bogus-tab-id`, cookies);
      if (driver.target === "web_new") {
        // Target for NEWFRONT-430 (PROF-026): unknown forwarded tabs
        // default to the general tab with the page rendered.
        expect(landed).toBe(account("general").replace(/\/$/, ""));
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
      // parameters like its siblings (landing on the slashed account form).
      expect(landed).toBe(`/${seed.workspaceSlug}/settings/account/api-tokens/?keep=3`);
    } else {
      expect(landed).toBe("/settings/profile/api-tokens/");
    }
  }
);
