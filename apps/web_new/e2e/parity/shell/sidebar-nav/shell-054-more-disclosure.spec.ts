// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the secondary-destinations disclosure
// auto-opens on their routes and persists.
// Observed on the running old app: the More disclosure starts closed,
// opens on demand, stays open across reloads, and opens itself when the
// user lands directly on a hosted member route (schedulers opens it
// with the Schedulers row active; the panel renders from the stored
// flag with no toggle change, and full-width member routes such as
// analytics render no sidebar at all). Row: SHELL-054.
import { test, expect } from "../../fixtures";
import { ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-054"];

test(
  specTitle(ROWS, "secondary disclosure auto-opens on its routes and persists"),
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

    await test.step("the disclosure starts closed and opens on demand", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.setMoreSectionOpen(false);
      expect(await driver.isMoreSectionOpen()).toBe(false);
      await driver.setMoreSectionOpen(true);
      expect(await driver.isMoreSectionOpen()).toBe(true);
      await expect.poll(() => driver.moreSectionLinks(), { timeout: 30_000 }).not.toEqual([]);
    });

    await test.step("the open state survives a reload", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isMoreSectionOpen()).toBe(true);
    });

    await test.step("landing on a member route auto-opens with the row active", async () => {
      await driver.setMoreSectionOpen(false);
      expect(await driver.isMoreSectionOpen()).toBe(false);
      await driver.openWorkspacePath(`/${seed.workspaceSlug}/schedulers/`);
      await expect.poll(() => driver.isMoreSectionOpen(), { timeout: 30_000 }).toBe(true);
      const schedulers = await driver.sidebarRowTone("Schedulers");
      const archives = await driver.sidebarRowTone("Archives");
      expect(schedulers.background !== archives.background || schedulers.color !== archives.color).toBe(true);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setMoreSectionOpen(false);
    });
  }
);
