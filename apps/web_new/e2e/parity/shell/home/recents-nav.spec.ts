// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-123): recent-row deep navigation. Row:
// SHELL-016. Behavior learned from the old dashboard in prose: work-item
// rows open an in-place preview, page rows land on the right scope, and
// project rows land on the project's work list. Oracle finding: the
// seeded OSS stack also previews work items in place (side panel in the
// fullscreen portal, home URL kept), so no cloud-only gap remains for
// this row; that observation is recorded in the inventory row as well.
import { test, expect } from "../../fixtures";
import {
  serverHomeIssues,
  serverSeedProject,
  serverSetTourCompleted,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-016"];

test(
  specTitle(ROWS, "recent rows navigate deep; work items preview in place"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["recents"]);
    const project = await serverSeedProject(seed.workspaceSlug, session);
    const issues = await serverHomeIssues(seed.workspaceSlug, project.id, session);
    const target = issues.find((issue) => /parity first issue/i.test(issue.name)) ?? issues[0];
    if (target === undefined) {
      test.skip(true, "seed carries no issues to visit");
      return;
    }

    await test.step("seed a recent visit then return home", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.homeOpenIssueDetail(seed.workspaceSlug, project.id, target.id);
      await driver.page.waitForTimeout(2_000);
      await driver.homeOpen(seed.workspaceSlug);
      await driver.homeWaitForWidgets();
      await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
    });

    await test.step("work-item row previews in place without leaving home", async () => {
      await driver.homeOpenRecentRow(target.name);
      await expect.poll(() => driver.homeIssuePreviewVisible(), { timeout: 30_000 }).toBe(true);
      expect(driver.page.url()).toContain(seed.workspaceSlug);
      // The preview resolves the clicked item: its name shows in the panel
      // once the item detail loads (the title is an editable field, so the
      // read combines rendered text with field values).
      await expect.poll(() => driver.homeIssuePreviewText(), { timeout: 30_000 }).toContain(target.name);
    });

    await test.step("project row lands on its work list", async () => {
      await driver.homeOpen(seed.workspaceSlug);
      await driver.homeWaitForWidgets();
      await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
      await driver.homeOpenRecentRow(project.name);
      // Client-side navigation does not refire document load, so wait for
      // the address itself (the list route compiles on first visit).
      await driver.page.waitForURL((url) => url.pathname.includes(`/projects/${project.id}/issues`), {
        timeout: 30_000,
      });
      expect(driver.page.url()).toContain(`/projects/${project.id}/issues`);
    });
  }
);
