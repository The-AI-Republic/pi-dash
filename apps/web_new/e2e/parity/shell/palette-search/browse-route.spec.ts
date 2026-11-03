// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level browse route (NEWFRONT-127). Row: SHELL-106 (negative
// row) — the browse route resolves a single work-item key to the same
// project-scoped detail view as in-project navigation, with no
// workspace-wide work-item browser.
import { test, expect } from "../../fixtures";
import { serverFirstWorkItemKey, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("workspace browse route", () => {
  test(
    specTitle(["SHELL-106"], "the browse route opens a project-scoped detail, not a workspace-wide browser"),
    { tag: specTags(["SHELL-106"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const { key, name } = await serverFirstWorkItemKey(seed.workspaceSlug, seed.projectId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      await test.step("the browse key opens that work item's detail view", async () => {
        await driver.openBrowseWorkItem(seed.workspaceSlug, key);
        await expect.poll(() => driver.currentUrlPath()).toBe(`/${seed.workspaceSlug}/browse/${key}`);
        await expect.poll(() => driver.hasVisibleText(name), { timeout: 120_000 }).toBe(true);
        expect(await driver.browseShowsWorkItemDetail()).toBe(true);
      });

      await test.step("no cross-project work-item browser is rendered", async () => {
        expect(await driver.browseShowsWorkspaceWideList()).toBe(false);
      });
    }
  );
});
