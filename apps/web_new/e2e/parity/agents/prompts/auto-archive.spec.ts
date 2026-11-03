// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the auto-archive row toggles idle
// archiving with a one-month default, reveals a duration picker with
// presets while enabled, persists every change, and funnels failures to
// one generic notice. Gap: the row promises viewers a read-only row, but
// the page gates non-admins to the not-authorized view (AGT-037), so the
// read-only half is unprovable.
// Row: AGT-034.
import { test, expect } from "../../fixtures";
import { ensureProject, parityProjectIdentifier, serverProjectAutomations } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { toastMessage } from "./support";

const ROWS = ["AGT-034"];

test(
  specTitle(ROWS, "auto-archive toggles with a one-month default, presets persist, failures toast once"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt34"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project (archiving off)", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT34 Project ${tag}`,
        parityProjectIdentifier("AG34")
      );
      expect((await serverProjectAutomations(workspaceSlug, created.id, ownerSession)).archive_in).toBe(0);
      return created;
    });

    await test.step("owner signs in and opens the automations page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(false);
    });

    await test.step("a disabled row shows the toggle only", async () => {
      const row = await driver.automationsArchiveRow();
      expect(row.toggleOn).toBe(false);
      expect(row.toggleDisabled).toBe(false);
      expect(row.pickerVisible).toBe(false);
    });

    await test.step("enabling defaults to one month and persists", async () => {
      await driver.automationsArchiveToggle();
      await expect.poll(async () => (await driver.automationsArchiveRow()).toggleOn, { timeout: 30_000 }).toBe(true);
      const row = await driver.automationsArchiveRow();
      expect(row.pickerVisible).toBe(true);
      expect(row.pickerLabel).toBe("1 month");
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(1);
    });

    await test.step("a preset delay persists", async () => {
      await driver.automationsArchiveSetPreset(6);
      await expect
        .poll(async () => (await driver.automationsArchiveRow()).pickerLabel, { timeout: 30_000 })
        .toBe("6 months");
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(6);
    });

    await test.step("disabling returns to zero and hides the picker", async () => {
      await driver.automationsArchiveToggle();
      await expect.poll(async () => (await driver.automationsArchiveRow()).toggleOn, { timeout: 30_000 }).toBe(false);
      expect((await driver.automationsArchiveRow()).pickerVisible).toBe(false);
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(0);
    });

    await test.step("a failing update toasts once and stores nothing", async () => {
      await driver.automationsFailUpdateOnce();
      await driver.automationsArchiveToggle();
      const message = await toastMessage(driver, "Error!");
      expect(message).toContain("Something went wrong. Please try again.");
      expect((await driver.automationsArchiveRow()).toggleOn).toBe(false);
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(0);
    });
  }
);
