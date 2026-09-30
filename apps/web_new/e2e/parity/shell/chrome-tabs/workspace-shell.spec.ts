// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the workspace route shell and the projects-area
// shell (NEWFRONT-126). The workspace route wraps every workspace page in
// the same chrome — top bar plus sidebar — behind a signed-in gate, and the
// projects-area shell keeps that sidebar and its overlay portal mounted
// while the visitor moves between home, the projects list and project
// pages. Rows: SHELL-001, SHELL-002.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-001", "SHELL-002"];

test(
  specTitle(ROWS, "workspace pages share one shell behind the sign-in gate"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("signed-out visitors are redirected away from workspace pages", async () => {
      await driver.page.goto(`/${seed.workspaceSlug}/`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => new URL(driver.page.url()).pathname, { timeout: 30_000 }).toBe("/");
      expect(await driver.sidebarPresent()).toBe(false);
    });

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("home renders the shared chrome", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      expect(await driver.sidebarPresent()).toBe(true);
      const controls = await driver.topBarControls();
      expect(controls.workspaceMenu).toBe(true);
      expect(controls.sidebarToggle).toBe(true);
      expect(await driver.portalPresent()).toBe(true);
    });

    await test.step("the projects list renders the same chrome", async () => {
      await driver.openProjectsList(seed.workspaceSlug);
      expect(await driver.sidebarPresent()).toBe(true);
      const controls = await driver.topBarControls();
      expect(controls.workspaceMenu).toBe(true);
      expect(await driver.portalPresent()).toBe(true);
    });

    await test.step("a project page renders the same chrome", async () => {
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      expect(await driver.sidebarPresent()).toBe(true);
      expect(await driver.portalPresent()).toBe(true);
    });

    await test.step("shell state carries across pages inside the area", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      const openWidth = await driver.sidebarWidth();
      expect(openWidth).toBeGreaterThan(200);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      await driver.openProjectsList(seed.workspaceSlug);
      expect(await driver.sidebarWidth()).toBe(0);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
    });
  }
);
