// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the sidebar project list shows
// placeholder rows while loading and a direct create action for empty
// workspaces.
// Observed on the running old app: while the project collection resolves
// the sidebar renders its chrome with placeholder blocks instead of
// project rows, then fills the rows in and unmounts the placeholders; a
// workspace with no projects offers a direct creation action instead of
// an empty list. Row: SHELL-049.
import { test, expect } from "../../fixtures";
import { deleteWorkspace, ensureWorkspace, ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-049"];
const EMPTY_SLUG = "parity-empty";

test(
  specTitle(ROWS, "project list placeholders while loading, create action when empty"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("rows fill in after the collection resolves", async () => {
      // Hold the collection response so the loading phase is observable,
      // then release it: placeholder blocks must render first, then give
      // way to the rows. The sidebar fills from the workspace project
      // collection endpoint.
      const pattern = "**/api/workspaces/*/projects/";
      await driver.page.route(pattern, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 8000));
        await route.continue();
      });
      try {
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.isSidebarOnScreen(), { timeout: 60_000 }).toBe(true);
        // Static chrome proves the shell rendered before the absence below
        // means anything.
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
        // The loading phase renders one placeholder block per skeleton row
        // while the collection is held.
        await expect.poll(() => driver.sidebarProjectPlaceholderCount(), { timeout: 30_000 }).toBe(4);
        const loading = await driver.sidebarLinkTexts();
        expect(loading).not.toContain(seed.projectName);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(seed.projectName);
        // Rows rendered: the placeholders unmount with them.
        await expect.poll(() => driver.sidebarProjectPlaceholderCount(), { timeout: 60_000 }).toBe(0);
      } finally {
        await driver.page.unroute(pattern);
      }
    });

    await test.step("an empty workspace offers creation instead of a list", async () => {
      await ensureWorkspace(session, "Parity Empty", EMPTY_SLUG);
      await driver.openWorkspacePath(`/${EMPTY_SLUG}/`);
      await expect.poll(() => driver.isSidebarOnScreen(), { timeout: 60_000 }).toBe(true);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.sidebarLinkTexts()).not.toContain(seed.projectName);
      expect(await driver.isCreateProjectVisible()).toBe(true);
      await deleteWorkspace(session, EMPTY_SLUG);
    });
  }
);
