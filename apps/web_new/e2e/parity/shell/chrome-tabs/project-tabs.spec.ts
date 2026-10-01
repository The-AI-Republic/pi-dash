// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the project switcher, the project header button and
// copy-location (NEWFRONT-126). The tab strip header offers a dropdown of
// joined projects that lands on the stored default tab, a truncating header
// with a role-gated quick-actions menu, and a copy entry that writes the
// active tab's absolute address to the clipboard with a notice, falling back
// to the tracking route when no tab is active. The strip only mounts in
// tabbed project-list mode, so each scenario enables it through the server
// preference first and restores it after. Rows: SHELL-072, SHELL-073,
// SHELL-074.
import { test, expect } from "../../fixtures";
import {
  createProject,
  deleteProject,
  getProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  patchProject,
  patchProjectUserProperties,
  patchWorkspaceUserProperties,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS_072 = ["SHELL-072"];
const ROWS_073 = ["SHELL-073"];
const ROWS_074 = ["SHELL-074"];

const SECOND_NAME = "Chrome Tabs Switch Target With A Deliberately Very Long Name";

test(
  specTitle(ROWS_072, "project switcher lists joined projects and lands on the stored default"),
  { tag: specTags(ROWS_072) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = originalProps["navigation_control_preference"] as string | undefined;
    const seedProject = await getProject(seed.workspaceSlug, seed.projectId, session);
    const cycleBefore = seedProject["cycle_view"];
    const navBefore = (
      (await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session))["preferences"] as
        | Record<string, unknown>
        | undefined
    )?.["navigation"] as Record<string, unknown> | undefined;

    await test.step("enable tabbed mode with cycles available and stored as the default", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await patchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: true });
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: { navigation: { default_tab: "cycles", hide_in_more_menu: [] } },
      });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
    });

    await test.step("the switcher lists joined projects and lands on the default tab", async () => {
      // Suffixed so an interrupted run's leftover project never collides here.
      const tag = Date.now()
        .toString(36)
        .toUpperCase()
        .replace(/[^A-Z0-9]/g, "")
        .slice(-4);
      const created = await createProject(seed.workspaceSlug, session, {
        name: SECOND_NAME,
        identifier: `CTS${tag}`,
      });
      const secondId = created["id"] as string;
      try {
        // The switcher renders the project list loaded with the page, so a
        // fresh load picks up the project created above.
        await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
        await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
        await driver.openProjectSwitcher();
        // Switcher rows shorten long names with a trailing ellipsis, so
        // the long second name matches by prefix, not in full.
        await expect
          .poll(
            async () => {
              const names = await driver.switcherOptionNames();
              const hasSeed = names.includes(seed.projectName);
              const hasSecond = names.some((n) => n.length > 10 && SECOND_NAME.startsWith(n.replace(/\.{3}$/, "")));
              return hasSeed && hasSecond;
            },
            { timeout: 15_000 }
          )
          .toBe(true);
        await driver.chooseSwitcherOption(SECOND_NAME.slice(0, 30));
        await expect.poll(() => driver.page.url(), { timeout: 15_000 }).toContain(secondId);
        await expect.poll(() => driver.page.url(), { timeout: 15_000 }).toContain("/cycles");
        // The header re-renders once the newly selected project loads, so
        // poll for its name instead of reading the stale entry one-shot.
        await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).toContain(SECOND_NAME);
      } finally {
        await deleteProject(seed.workspaceSlug, secondId, session);
      }
    });

    await test.step("restore mode, flag and default", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: originalMode ?? "ACCORDION",
      });
      await patchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: cycleBefore });
      await patchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        preferences: {
          navigation: {
            default_tab:
              typeof navBefore?.["default_tab"] === "string" ? (navBefore["default_tab"] as string) : "work_items",
            hide_in_more_menu: Array.isArray(navBefore?.["hide_in_more_menu"])
              ? (navBefore["hide_in_more_menu"] as string[])
              : [],
          },
        },
      });
    });
  }
);

test(
  specTitle(ROWS_073, "project header truncates with hover reveal beside role-gated actions"),
  { tag: specTags(ROWS_073) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = originalProps["navigation_control_preference"] as string | undefined;

    await test.step("enable tabbed mode for the strip", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
    });

    await test.step("long names truncate with full text on hover", async () => {
      const tag = Date.now()
        .toString(36)
        .toUpperCase()
        .replace(/[^A-Z0-9]/g, "")
        .slice(-4);
      const created = await createProject(seed.workspaceSlug, session, {
        name: SECOND_NAME,
        identifier: `CTH${tag}`,
      });
      const secondId = created["id"] as string;
      try {
        await driver.openProjectTab(seed.workspaceSlug, secondId, "issues");
        await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).toContain(SECOND_NAME);
        expect(await driver.projectHeaderTruncated()).toBe(true);
        const before = await driver.projectNameVisibleCount(SECOND_NAME);
        await driver.hoverProjectHeader();
        await expect
          .poll(() => driver.projectNameVisibleCount(SECOND_NAME), { timeout: 15_000 })
          .toBeGreaterThan(before);
      } finally {
        await deleteProject(seed.workspaceSlug, secondId, session);
      }
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
    });

    await test.step("each entry is offered to the owner and navigates or dialogs", async () => {
      expect(await driver.projectHeaderText()).toContain(seed.projectName);
      await driver.openProjectActions();
      const entries = await driver.projectActionNames();
      expect(entries).toEqual(expect.arrayContaining(["Publish project", "Copy link", "Archives", "Settings"]));
      expect(entries).not.toContain("Leave project");
      await driver.page.keyboard.press("Escape");

      await driver.openProjectActions();
      await driver.clickProjectAction("Archives");
      await expect.poll(() => driver.page.url(), { timeout: 15_000 }).toContain("/archives/issues");
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");

      await driver.openProjectActions();
      await driver.clickProjectAction("Settings");
      await expect.poll(() => driver.page.url(), { timeout: 15_000 }).toContain(`/settings/projects/${seed.projectId}`);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");

      await driver.openProjectActions();
      await driver.clickProjectAction("Publish project");
      await expect.poll(() => driver.projectActionDialogHeading(), { timeout: 15_000 }).toBe("Publish project");
      await driver.page.keyboard.press("Escape");
    });

    await test.step("restore the original rendering mode", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: originalMode ?? "ACCORDION",
      });
    });
  }
);

test(
  specTitle(ROWS_074, "copy-location writes the active tab route with a tracking fallback"),
  { tag: specTags(ROWS_074) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = originalProps["navigation_control_preference"] as string | undefined;

    await test.step("enable tabbed mode for the strip", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
    });

    await test.step("copy-location writes the active tab route with a notice", async () => {
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await driver.openProjectActions();
      await driver.clickProjectAction("Copy link");
      // The entry copies the absolute address; match the route suffix so
      // the assertion holds on any host running the suite.
      await expect
        .poll(
          async () =>
            (await driver.readClipboardText()).endsWith(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`),
          { timeout: 15_000 }
        )
        .toBe(true);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/copied/i);
    });

    await test.step("with no active tab the copy falls back to the tracking route", async () => {
      // Disabling the Pages feature drops its tab while the route stays
      // addressable, so the header renders with nothing highlighted and
      // the copy falls back to the tracking route.
      const pageViewBefore = (await getProject(seed.workspaceSlug, seed.projectId, session))["page_view"];
      await patchProject(seed.workspaceSlug, seed.projectId, session, { page_view: false });
      try {
        await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}/pages`);
        await driver.page.waitForLoadState("domcontentloaded");
        await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
        expect(await driver.activeTabName()).toBeNull();
        await driver.openProjectActions();
        await driver.clickProjectAction("Copy link");
        await expect
          .poll(
            async () =>
              (await driver.readClipboardText()).endsWith(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`),
            { timeout: 15_000 }
          )
          .toBe(true);
      } finally {
        await patchProject(seed.workspaceSlug, seed.projectId, session, { page_view: pageViewBefore });
      }
    });

    await test.step("restore the original rendering mode", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: originalMode ?? "ACCORDION",
      });
    });
  }
);

test(
  specTitle(ROWS_074, "copy-location failure toasts distinctly"),
  { tag: specTags(ROWS_074) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = originalProps["navigation_control_preference"] as string | undefined;

    await test.step("enable tabbed mode for the strip", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
    });

    await test.step("a refused clipboard write toasts a failure", async () => {
      // No clipboard grant in this context, so the write is refused and
      // the entry reports the failure instead of a copy.
      await driver.openProjectActions();
      await driver.clickProjectAction("Copy link");
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/copy failed|couldn't copy/i);
    });

    await test.step("restore the original rendering mode", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: originalMode ?? "ACCORDION",
      });
    });
  }
);
