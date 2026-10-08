// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): module dropdown — project-scoped
// multi-select with chips, search, removal, no inline creation.
// Rows: ISS-213 (module dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCreateIssueFull,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverDeleteModule,
  serverCleanupProject,
  serverIssue,
  serverProjectModules,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-213"], "module dropdown multi-selects, searches and removes"),
  { tag: specTags(["ISS-213"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 modules ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // The Modules row only renders when the project's module view is on, and
    // the seed project keeps it off — so the scenario owns its project.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { moduleView: true },
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const alphaId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} alpha`, session);
    const betaId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} beta`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("empty issues offer project modules behind a No module placeholder", async () => {
        await expect.poll(() => driver.propertyValueText("Modules"), { timeout: 15_000 }).toContain("No module");
        const modules = await serverProjectModules(seed.workspaceSlug, projectId, session);
        expect(modules.some((m) => m.id === alphaId)).toBe(true);
        await driver.propertyOpenPicker("Modules");
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes(`${tag} alpha`))).toBe(true);
        expect(options.some((o) => o.includes(`${tag} beta`))).toBe(true);
        await driver.pickerClickOutside();
      });

      await test.step("search narrows options and a miss shows the empty message", async () => {
        await driver.propertyOpenPicker("Modules");
        await driver.pickerSearch(`${tag} alpha`);
        expect(await driver.pickerOptionTexts()).toEqual([expect.stringContaining(`${tag} alpha`)]);
        await driver.pickerSearch(`${tag} no-such-module`);
        expect(await driver.pickerOptionTexts()).toEqual([]);
        expect(await driver.pickerEmptyText()).toContain("No matching results");
        await driver.pickerPressEscape();
        await driver.pickerClickOutside();
      });

      await test.step("picks accumulate server-side and render in the row", async () => {
        await driver.propertyOpenPicker("Modules");
        await driver.pickerPick(`${tag} alpha`);
        expect(await driver.pickerOpen()).toBe(true);
        await driver.pickerPick(`${tag} beta`);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).module_ids, {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining([alphaId, betaId]));
        await expect.poll(() => driver.propertyValueText("Modules"), { timeout: 15_000 }).toContain(`${tag} alpha`);
        await driver.pickerClickOutside();
      });

      await test.step("unpicking a module removes it server-side", async () => {
        await driver.propertyOpenPicker("Modules");
        await driver.pickerPick(`${tag} alpha`);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).module_ids, {
            timeout: 15_000,
          })
          .toEqual([betaId]);
        await driver.pickerPick(`${tag} beta`);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).module_ids, {
            timeout: 15_000,
          })
          .toEqual([]);
        await expect.poll(() => driver.propertyValueText("Modules"), { timeout: 15_000 }).toContain("No module");
        await driver.pickerClickOutside();
      });
    } finally {
      // The issue is module-free again, so both scenario modules delete
      // cleanly; the project delete cascades anything left behind.
      await serverDeleteModule(seed.workspaceSlug, projectId, alphaId, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, projectId, betaId, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
