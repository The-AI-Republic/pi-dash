// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the inline cycle and module pickers,
// offered only where the project's views enable them. Rows: DRAFT-018,
// DRAFT-019. Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  serverCreateCycle,
  serverCreateDraft,
  serverCreateModule,
  serverDraftRecord,
  setProjectViewFlags,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs } from "./support";

function isoIn(days: number): string {
  const at = new Date();
  at.setDate(at.getDate() + days);
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
}

test(
  specTitle(["DRAFT-018"], "cycle attaches inline where cycle views apply"),
  { tag: specTags(["DRAFT-018"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d18");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D18 Cycle ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const cycleName = `Cycle ${harness.tag}`;
    const cycleId = await serverCreateCycle(workspaceSlug, projectId, cycleName, isoIn(-7), isoIn(60), owner.cookie);

    await test.step("no cycle control until cycle views apply", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftCyclePickerVisible(name)).toBe(false);
    });

    await test.step("enabling cycle views offers the picker", async () => {
      await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { cycle_view: true });
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftCyclePickerVisible(name), { timeout: 60_000 }).toBe(true);
    });

    await test.step("attaching a cycle renders and persists", async () => {
      await driver.draftOpenCyclePicker(name);
      const options = await driver.pickerOptionTexts();
      expect(options).toContain(cycleName);
      await driver.pickerPick(cycleName);
      await expect.poll(() => driver.draftCycleText(name), { timeout: 30_000 }).toContain(cycleName);
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).cycleId, {
          timeout: 30_000,
        })
        .toBe(cycleId);
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftCycleText(name), { timeout: 60_000 }).toContain(cycleName);
    });
  }
);

test(
  specTitle(["DRAFT-019"], "modules attach inline where module views apply"),
  { tag: specTags(["DRAFT-019"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d19");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D19 Modules ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const moduleName = `Module ${harness.tag}`;
    const moduleId = await serverCreateModule(workspaceSlug, projectId, moduleName, owner.cookie);

    await test.step("no modules control until module views apply", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftModulePickerVisible(name)).toBe(false);
    });

    await test.step("enabling module views offers the picker", async () => {
      await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { module_view: true });
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftModulePickerVisible(name), { timeout: 60_000 }).toBe(true);
    });

    await test.step("attaching a module renders and persists", async () => {
      await driver.draftOpenModulePicker(name);
      const options = await driver.pickerOptionTexts();
      expect(options).toContain(moduleName);
      await driver.pickerPick(moduleName);
      await driver.pickerPressEscape();
      await expect.poll(() => driver.draftModuleText(name), { timeout: 30_000 }).toContain(moduleName);
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).moduleIds, {
          timeout: 30_000,
        })
        .toContain(moduleId);
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftModuleText(name), { timeout: 60_000 }).toContain(moduleName);
    });
  }
);
