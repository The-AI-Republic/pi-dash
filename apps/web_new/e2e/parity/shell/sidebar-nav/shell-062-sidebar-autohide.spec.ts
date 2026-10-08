// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the sidebar hides itself on the
// notifications route and collapses on narrow screens after navigation.
// Observed on the running old app: the notifications route renders
// full-width with no app sidebar, and on a narrow viewport following a
// sidebar link closes the sidebar. Row: SHELL-062.
import { test, expect } from "../../fixtures";
import { ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-062"];

test(
  specTitle(ROWS, "sidebar hides on notifications and collapses on mobile navigation"),
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

    await test.step("notifications render full-width with no app sidebar", async () => {
      await driver.openWorkspacePath(`/${seed.workspaceSlug}/notifications/`);
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 60_000 })
        .toBe(`/${seed.workspaceSlug}/notifications/`);
      expect(await driver.isSidebarOnScreen()).toBe(false);
    });

    await test.step("tapping a link on a narrow screen closes the sidebar", async () => {
      await driver.page.setViewportSize({ width: 390, height: 844 });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isSidebarOnScreen()).toBe(true);
      await driver.openSidebarLink("Home");
      await expect.poll(() => driver.isSidebarOnScreen(), { timeout: 30_000 }).toBe(false);
    });
  }
);
