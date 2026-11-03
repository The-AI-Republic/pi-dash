// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for the project tab strip (NEWFRONT-126). In tabbed
// project-list mode the strip renders each permitted, feature-enabled tab
// in a fixed order, keeps the active tab highlighted through nested routes
// and work-item detail pages, and persists per-tab default and hide choices
// per member, with hidden tabs retrievable behind the overflow menu. Bare
// project addresses rewrite to the stored default tab (subsuming the old
// not-found bug NEWFRONT-141, which the oracle scenario proved only as
// degenerate behavior). Narrow widths pool trailing tabs behind the
// overflow menu on the production oracle (dev-only NEWFRONT-142 clipped
// without pooling); the pooling scenario is a parity target. The
// strip only mounts in tabbed mode, so each width-sensitive scenario
// enables it and every feature flag through the server first and restores
// both after. Rows: SHELL-075, SHELL-076, SHELL-077, SHELL-078, SHELL-079.
// Target alignment (NEWFRONT-295 gate): the SHELL-077 scenario asserts
// the intended rewrite, which web_new implements; the old app still
// renders not-found there, so that row has no oracle.
import { test, expect } from "../../fixtures";
import {
  getProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  patchProject,
  patchProjectUserProperties,
  patchWorkspaceUserProperties,
  serverIssueKeys,
  serverSetTourCompleted,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS_075 = ["SHELL-075"];
const ROWS_076 = ["SHELL-076"];
const ROWS_077 = ["SHELL-077"];
const ROWS_078 = ["SHELL-078"];
const ROWS_079 = ["SHELL-079"];

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
  specTitle(ROWS_075, "tabs render in fixed order and follow feature gating"),
  { tag: specTags(ROWS_075) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    await serverSetTourCompleted(session, true);
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const projectBefore = await getProject(seed.workspaceSlug, seed.projectId, session);
    const flagsBefore: Record<string, unknown> = {};
    for (const key of FLAG_KEYS) flagsBefore[key] = projectBefore[key];

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

    await test.step("restore mode and flags", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: modeBefore ?? "ACCORDION",
      });
      await patchProject(seed.workspaceSlug, seed.projectId, session, flagsBefore);
    });
  }
);

test(
  specTitle(ROWS_076, "active tab survives nesting and work-item detail pages"),
  { tag: specTags(ROWS_076) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    await serverSetTourCompleted(session, true);
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const projectBefore = await getProject(seed.workspaceSlug, seed.projectId, session);
    const flagsBefore: Record<string, unknown> = {};
    for (const key of FLAG_KEYS) flagsBefore[key] = projectBefore[key];

    await test.step("enable tabbed mode and every feature flag", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      const enable: Record<string, unknown> = {};
      for (const key of FLAG_KEYS) enable[key] = true;
      await patchProject(seed.workspaceSlug, seed.projectId, session, enable);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectTabs(), { timeout: 60_000 }).not.toEqual([]);
    });

    await test.step("the active tab survives nesting and work-item detail pages", async () => {
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Work Items");
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "pages");
      await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Pages");

      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Work Items");

      // Row clicks open the peek overlay (issues-area behavior), so reach
      // the detail page through its address: project identifier plus the
      // issue's per-project sequence.
      const keys = await serverIssueKeys(seed.workspaceSlug, seed.projectId, session);
      const seedKey = keys.find((k) => seed.issueNames.includes(k.name)) ?? keys[0];
      expect(seedKey).not.toBeUndefined();
      const identifier = projectBefore["identifier"] as string;
      // Re-assert tabbed mode right before the detail load: agent-mates
      // share this member's rendering mode and the fresh page reads it.
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await driver.page.goto(`/${seed.workspaceSlug}/browse/${identifier}-${seedKey?.sequence_id}`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/browse/");
      // The detail header mounts its own strip once the issue loads, so
      // poll for the tracked tab instead of reading it one-shot, with a
      // generous budget for shared-stack contention.
      await expect.poll(() => driver.activeTabName(), { timeout: 60_000 }).toBe("Work Items");
    });

    await test.step("restore mode and flags", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: modeBefore ?? "ACCORDION",
      });
      await patchProject(seed.workspaceSlug, seed.projectId, session, flagsBefore);
    });
  }
);

test(
  specTitle(ROWS_077, "bare project URL rewrites to the stored default tab"),
  { tag: specTags(ROWS_077) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    await serverSetTourCompleted(session, true);
    const bare = `/${seed.workspaceSlug}/projects/${seed.projectId}`;
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const cycleBefore = (await getProject(seed.workspaceSlug, seed.projectId, session))["cycle_view"];

    await test.step("the bare address rewrites to the stored default tab", async () => {
      // Intended behavior, subsuming NEWFRONT-141: the stored default
      // wins when it names an available tab, and the landed tab
      // highlights. The strip only mounts in tabbed mode and cycles is
      // feature-gated, so enable both explicitly: the seed default
      // leaves them off and the redirect would otherwise (correctly)
      // fall back to tracking with no active tab.
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await patchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: true });
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "cycles", hide_in_more_menu: [] } },
      });
      try {
        await driver.page.goto(bare);
        await expect
          .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
          .toBe(`${bare}/cycles`);
        await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Cycles");
        expect(await driver.errorNoticeVisible()).toBe(false);
      } finally {
        await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        });
      }
    });

    await test.step("unknown stored keys fall back to the tracking tab", async () => {
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "nope", hide_in_more_menu: [] } },
      });
      try {
        await driver.page.goto(bare);
        await expect
          .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
          .toBe(`${bare}/issues`);
        await expect.poll(() => driver.activeTabName(), { timeout: 30_000 }).toBe("Work Items");
      } finally {
        await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        });
        await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
          navigation_control_preference: modeBefore ?? "ACCORDION",
        });
        await patchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: cycleBefore });
      }
    });
  }
);

test(
  specTitle(ROWS_078, "narrow widths pool trailing tabs into the overflow menu"),
  { tag: specTags(ROWS_078) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    await serverSetTourCompleted(session, true);
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const projectBefore = await getProject(seed.workspaceSlug, seed.projectId, session);
    const flagsBefore: Record<string, unknown> = {};
    for (const key of FLAG_KEYS) flagsBefore[key] = projectBefore[key];
    const navBefore = navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session));

    await test.step("enable tabbed mode and every feature flag", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      const enable: Record<string, unknown> = {};
      for (const key of FLAG_KEYS) enable[key] = true;
      await patchProject(seed.workspaceSlug, seed.projectId, session, enable);
      // A hidden tab mounts the trigger regardless of width, so start from
      // neutral tab preferences: agent-mates share this member's prefs.
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
      });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectTabs(), { timeout: 60_000 }).not.toEqual([]);
    });

    await test.step("narrow widths pool trailing tabs into the overflow menu", async () => {
      // Production truth (NEWFRONT-246): narrowing the strip pools trailing
      // tabs behind the overflow trigger. The old bug: scenario asserted the
      // dev oracle, where the resize measurement never recomputes (StrictMode
      // double-mount disconnects the tab strip's ResizeObserver), so widths
      // clipped without pooling there — recorded as dev-only on
      // NEWFRONT-142. The row and scenario follow production, which is what
      // users run.
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      const wide = await driver.projectTabs();
      expect(wide.length).toBeGreaterThan(0);
      expect(await driver.overflowTriggerPresent()).toBe(false);
      await driver.setViewportSize(500, 800);
      await driver.page.waitForTimeout(2_000);
      await expect.poll(() => driver.overflowTriggerPresent(), { timeout: 30_000 }).toBe(true);
      await driver.openOverflowMenu();
      const pooled = await driver.overflowRowNames();
      expect(pooled.length).toBeGreaterThan(0);
      expect(pooled).toContain("Schedulers");
      await driver.setViewportSize(1280, 720);
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

test(
  specTitle(ROWS_079, "default and hide choices persist per member"),
  { tag: specTags(ROWS_079) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    await serverSetTourCompleted(session, true);
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    const projectBefore = await getProject(seed.workspaceSlug, seed.projectId, session);
    const flagsBefore: Record<string, unknown> = {};
    for (const key of FLAG_KEYS) flagsBefore[key] = projectBefore[key];
    const navBefore = navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session));

    await test.step("enable tabbed mode and every feature flag", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      const enable: Record<string, unknown> = {};
      for (const key of FLAG_KEYS) enable[key] = true;
      await patchProject(seed.workspaceSlug, seed.projectId, session, enable);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectTabs(), { timeout: 60_000 }).not.toEqual([]);
    });

    await test.step("setting a default persists and marks the tab", async () => {
      // The stored default drives switcher landings (proven for SHELL-072);
      // this step proves the context menu writes it and marks the tab.
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

      await driver.rightClickTab("Cycles");
      await expect
        .poll(() => driver.contextMenuItems(), { timeout: 15_000 })
        .toEqual(expect.arrayContaining(["Clear default", "Hide in more menu"]));
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
    });

    await test.step("hiding moves the tab behind the overflow menu with restore", async () => {
      await driver.rightClickTab("Pages");
      await driver.clickContextMenuItem("Hide in more menu");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 15_000 })
        .not.toContain("Pages");
      await expect
        .poll(
          async () => navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).hidden,
          { timeout: 15_000 }
        )
        .toContain("pages");
      await driver.openOverflowMenu();
      await expect.poll(() => driver.overflowRowNames(), { timeout: 15_000 }).toContain("Pages");
      await driver.restoreOverflowTab("Pages");
      await expect
        .poll(async () => (await driver.projectTabs()).map((t) => t.name), { timeout: 15_000 })
        .toContain("Pages");
      await expect
        .poll(
          async () => navigationOf(await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).hidden,
          { timeout: 15_000 }
        )
        .not.toContain("pages");
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
