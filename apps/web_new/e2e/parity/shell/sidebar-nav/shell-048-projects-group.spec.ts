// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the sidebar projects group bundles
// drafts, an aggregate work-items row and joined projects under a
// collapsing disclosure that auto-opens on related routes.
// Observed on the running old app: the Projects group lists the Drafts
// row, the aggregate Work Items row and every joined project; collapsing
// it survives client-side navigation, while a full page load re-opens
// the group; visiting a drafts or views URL expands the group again with
// the matching row active and visible. Row: SHELL-048.
import { test, expect } from "../../fixtures";
import { ownerSession, patchUserProperties } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-048"];

test(
  specTitle(ROWS, "projects group bundles drafts, views and projects under a collapsing disclosure"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    await test.step("prepare server session", async () => {
      const session = await ownerSession(seed);
      // Uncapped baseline: an interrupted overflow run may have left a cap.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
    });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the group bundles aggregates and joined projects", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setProjectsGroupOpen(true);
      await expect
        .poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(["Drafts", "Work Items", seed.projectName]));
    });

    await test.step("collapsing survives client-side navigation", async () => {
      await driver.setProjectsGroupOpen(false);
      expect(await driver.isProjectsGroupOpen()).toBe(false);
      await driver.openSidebarLink("Home");
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isProjectsGroupOpen()).toBe(false);
    });

    await test.step("a full load re-opens the group", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isProjectsGroupOpen()).toBe(true);
    });

    await test.step("visiting a drafts URL expands the group", async () => {
      await driver.setProjectsGroupOpen(false);
      expect(await driver.isProjectsGroupOpen()).toBe(false);
      await driver.openWorkspacePath(`/${seed.workspaceSlug}/drafts/`);
      await expect.poll(() => driver.isProjectsGroupOpen(), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Drafts");
      const drafts = await driver.sidebarRowTone("Drafts");
      const home = await driver.sidebarRowTone("Home");
      expect(drafts.background).not.toBe("rgba(0, 0, 0, 0)");
      expect(home.background).toBe("rgba(0, 0, 0, 0)");
    });

    await test.step("visiting a views URL expands the group", async () => {
      await driver.setProjectsGroupOpen(false);
      expect(await driver.isProjectsGroupOpen()).toBe(false);
      await driver.openWorkspacePath(`/${seed.workspaceSlug}/workspace-views/all-issues/`);
      await expect.poll(() => driver.isProjectsGroupOpen(), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Work Items");
      const views = await driver.sidebarRowTone("Work Items");
      const home = await driver.sidebarRowTone("Home");
      expect(views.background).not.toBe("rgba(0, 0, 0, 0)");
      expect(home.background).toBe("rgba(0, 0, 0, 0)");
      await driver.setProjectsGroupOpen(true);
    });
  }
);
