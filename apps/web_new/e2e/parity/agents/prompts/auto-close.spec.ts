// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the auto-close row toggles idle
// closing with a one-month default plus the first cancelled state as
// the target; the duration presets and the cancelled-only state picker
// persist; disabling clears both; failures toast once. With no
// cancelled state left, the picker shows its placeholder.
// Row: AGT-035.
import { test, expect } from "../../fixtures";
import {
  createServerState,
  deleteServerState,
  ensureProject,
  parityProjectIdentifier,
  serverProjectAutomations,
  serverProjectCancelledStates,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { toastMessage } from "./support";

const ROWS = ["AGT-035"];

test(
  specTitle(ROWS, "auto-close toggles with a delay and a cancelled-state target; failures toast once"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt35"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project (closing off)", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT35 Project ${tag}`,
        parityProjectIdentifier("AG35")
      );
      const settings = await serverProjectAutomations(workspaceSlug, created.id, ownerSession);
      expect(settings.close_in).toBe(0);
      expect(settings.default_state).toBeNull();
      return created;
    });

    await test.step("owner signs in and opens the automations page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(false);
    });

    await test.step("enabling defaults to one month plus the first cancelled state", async () => {
      const [first] = await serverProjectCancelledStates(workspaceSlug, project.id, ownerSession);
      if (first === undefined) throw new Error("[parity] expected a cancelled state.");
      await driver.automationsCloseToggle();
      await expect.poll(async () => (await driver.automationsCloseRow()).toggleOn, { timeout: 30_000 }).toBe(true);
      const row = await driver.automationsCloseRow();
      expect(row.pickerVisible).toBe(true);
      expect(row.pickerLabel).toBe("1 month");
      expect(row.stateLabel).toBe(first.name);
      // A fresh project holds a single cancelled state, so the picker
      // stays disabled until a second one exists.
      expect(row.statePickerDisabled).toBe(true);
      const settings = await serverProjectAutomations(workspaceSlug, project.id, ownerSession);
      expect(settings.close_in).toBe(1);
      expect(settings.default_state).toBe(first.id);
    });

    await test.step("a preset delay persists", async () => {
      await driver.automationsCloseSetPreset(3);
      await expect
        .poll(async () => (await driver.automationsCloseRow()).pickerLabel, { timeout: 30_000 })
        .toBe("3 months");
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).close_in).toBe(3);
    });

    await test.step("a second cancelled state enables the state picker", async () => {
      const extra = await createServerState(workspaceSlug, project.id, ownerSession, {
        name: `AGT35 Dropped ${tag}`,
        group: "cancelled",
        color: "#6b7280",
      });
      await driver.automationsOpen(workspaceSlug, project.id);
      await expect
        .poll(async () => (await driver.automationsCloseRow()).statePickerDisabled, { timeout: 30_000 })
        .toBe(false);
      const options = await driver.automationsCloseStateOptions();
      const cancelled = await serverProjectCancelledStates(workspaceSlug, project.id, ownerSession);
      expect(options.sort()).toEqual(cancelled.map((state) => state.name).sort());
      await driver.automationsCloseSetState(extra.name);
      await expect
        .poll(async () => (await driver.automationsCloseRow()).stateLabel, { timeout: 30_000 })
        .toBe(extra.name);
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).default_state).toBe(extra.id);
    });

    await test.step("disabling clears the delay and the target", async () => {
      await driver.automationsCloseToggle();
      await expect.poll(async () => (await driver.automationsCloseRow()).toggleOn, { timeout: 30_000 }).toBe(false);
      expect((await driver.automationsCloseRow()).pickerVisible).toBe(false);
      const settings = await serverProjectAutomations(workspaceSlug, project.id, ownerSession);
      expect(settings.close_in).toBe(0);
      expect(settings.default_state).toBeNull();
    });

    await test.step("a failing update toasts once and stores nothing", async () => {
      await driver.automationsFailUpdateOnce();
      await driver.automationsCloseToggle();
      const message = await toastMessage(driver, "Error!");
      expect(message).toContain("Something went wrong. Please try again.");
      expect((await driver.automationsCloseRow()).toggleOn).toBe(false);
      const settings = await serverProjectAutomations(workspaceSlug, project.id, ownerSession);
      expect(settings.close_in).toBe(0);
      expect(settings.default_state).toBeNull();
    });

    await test.step("with no cancelled state the picker shows its placeholder", async () => {
      const cancelled = await serverProjectCancelledStates(workspaceSlug, project.id, ownerSession);
      for (const state of cancelled) {
        await deleteServerState(workspaceSlug, project.id, state.id, ownerSession);
      }
      expect(await serverProjectCancelledStates(workspaceSlug, project.id, ownerSession)).toEqual([]);
      await driver.automationsOpen(workspaceSlug, project.id);
      await driver.automationsCloseToggle();
      await expect.poll(async () => (await driver.automationsCloseRow()).toggleOn, { timeout: 30_000 }).toBe(true);
      const row = await driver.automationsCloseRow();
      expect(row.stateLabel).toBe("State");
      expect(row.statePickerDisabled).toBe(true);
    });
  }
);
