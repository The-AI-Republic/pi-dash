// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the project tab strip (NEWFRONT-126). In tabbed
// project-list mode the strip renders each permitted, feature-enabled tab
// in a fixed order, keeps the active tab highlighted through nested routes
// and work-item detail pages, rewrites bare project addresses to the stored
// default tab, pools whatever does not fit into an overflow menu, and
// persists per-tab default and hide choices per member. The strip only
// mounts in tabbed mode, so these scenarios enable it and every feature
// flag through the server first and restore both after. Rows: SHELL-075,
// SHELL-076, SHELL-077, SHELL-078, SHELL-079.
import { test, expect } from "../../fixtures";
import {
  getProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  patchProject,
  patchProjectUserProperties,
  patchWorkspaceUserProperties,
  serverIssueNames,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-075", "SHELL-076", "SHELL-077", "SHELL-078", "SHELL-079"];

const CANONICAL_ORDER = ["Work Items", "Cycles", "Modules", "Views", "Pages", "Intake", "Schedulers"];

const FLAG_KEYS = ["cycle_view", "module_view", "issue_views_view", "page_view", "inbox_view"];

function navigationOf(props: Record<string, unknown>): { defaultTab: string; hidden: string[] } {
  const prefs = (props["preferences"] ?? {}) as Record<string, unknown>;
  const nav = (prefs["navigation"] ?? {}) as Record<string, unknown>;
  return {
    defaultTab: typeof nav["default_tab"] === "string" ? (nav["default_tab"] as string) : "work_items",
    hidden: Array.isArray(nav["hide_in_more_menu"]) ? (nav["hide_in_more_menu"] as string[]) : [],
  };
}

test(
  specTitle(ROWS, "project tab strip gating, order, routing, overflow and preferences"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);

    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const projectBefore = await getProject(seed.workspaceSlug, seed.projectId, session);
    const flagsBefore: Record<string, unknown> = {};
    for (const key of FLAG_KEYS) flagsBefore[key] = projectBefore[key];
    const prefsBefore = await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session);
    const navBefore = navigationOf(prefsBefore);

    await test.step("enable tabbed mode and every feature flag", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      const enable: Record<string, unknown> = {};
      for (const key of FLAG_KEYS) enable[key] = true;
      await patchProject(seed.workspaceSlug, seed.projectId, session, enable);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectTabs(), { timeout: 60_000 }).not.toEqual([]);
    });

    await test.step("tabs render permitted entries in their fixed order", async () => {
      const tabs = await driver.projectTabs();
      const names = tabs.map((t) => t.name);
      expect(names[0]).toBe("Work Items");
      const canonical = names.filter((n) => CANONICAL_ORDER.includes(n));
      const sorted = [...canonical].sort((a, b) => CANONICAL_ORDER.indexOf(a) - CANONICAL_ORDER.indexOf(b));
      expect(canonical).toEqual(sorted);
      for (const tab of tabs) {
        expect(tab.href).toContain(`/projects/${seed.projectId}/`);
      }
    });

    await test.step("a disabled feature drops its tab", async () => {
      await patchProject(seed.workspaceSlug, seed.projectId, session, { page_view: false });
      await driver.page.reload();
      await driver.page.waitForLoadState("domcontentloaded");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 30_000 })
        .not.toContain("Pages");
      await patchProject(seed.workspaceSlug, seed.projectId, session, { page_view: true });
      await driver.page.reload();
      await driver.page.waitForLoadState("domcontentloaded");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 30_000 })
        .toContain("Pages");
    });

    await test.step("the active tab survives nesting and work-item detail pages", async () => {
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Work Items");
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "pages");
      await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Pages");

      const serverNames = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      const detailName = seed.issueNames.find((n) => serverNames.includes(n)) ?? serverNames[0] ?? "";
      expect(detailName).not.toBe("");
      const visible = await driver.visibleIssueNames();
      expect(visible).toContain(detailName);
      const row = driver.page
        .locator("main main")
        .getByRole("link", { name: new RegExp(detailName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) })
        .first();
      await row.click();
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/browse/");
      expect(await driver.activeTabName()).toBe("Work Items");
    });

    await test.step("bare project addresses rewrite to the stored default tab", async () => {
      await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/issues");

      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "cycles", hide_in_more_menu: [] } },
      });
      await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/cycles");

      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "no_such_tab", hide_in_more_menu: [] } },
      });
      await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/issues");

      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: navBefore.defaultTab, hide_in_more_menu: navBefore.hidden } },
      });
    });

    await test.step("narrow widths pool trailing tabs into the overflow menu", async () => {
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await driver.setViewportSize(700, 800);
      await driver.openOverflowMenu();
      const rows = await driver.overflowRowNames();
      expect(rows.length).toBeGreaterThan(0);
      expect(await driver.activeTabName()).toBe("Work Items");
      await driver.page.keyboard.press("Escape");
      await driver.setViewportSize(1280, 720);
    });

    await test.step("default and hide choices persist per member", async () => {
      await driver.rightClickTab("Cycles");
      await expect
        .poll(() => driver.contextMenuItems(), { timeout: 15_000 })
        .toEqual(expect.arrayContaining(["Set as default", "Hide in more menu"]));
      await driver.clickContextMenuItem("Set as default");
      await expect
        .poll(
          async () =>
            navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).defaultTab,
          {
            timeout: 15_000,
          }
        )
        .toBe("cycles");
      await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/cycles");

      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "cycles");
      await driver.rightClickTab("Cycles");
      await driver.clickContextMenuItem("Clear default");
      await expect
        .poll(
          async () =>
            navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).defaultTab,
          {
            timeout: 15_000,
          }
        )
        .toBe("work_items");

      await driver.rightClickTab("Pages");
      await driver.clickContextMenuItem("Hide in more menu");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 15_000 })
        .not.toContain("Pages");
      await driver.openOverflowMenu();
      await expect.poll(() => driver.overflowRowNames(), { timeout: 15_000 }).toContain("Pages");
      await driver.restoreOverflowTab("Pages");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 15_000 })
        .toContain("Pages");
    });

    await test.step("restore mode, flags and tab preferences", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: modeBefore ?? "ACCORDION",
      });
      await patchProject(seed.workspaceSlug, seed.projectId, session, flagsBefore);
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: navBefore.defaultTab, hide_in_more_menu: navBefore.hidden } },
      });
      const restored = navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session));
      expect(restored).toEqual(navBefore);
    });
  }
);
