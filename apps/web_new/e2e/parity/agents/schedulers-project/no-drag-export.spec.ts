// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): the firing calendar offers no
// drag-to-create, drag-to-reschedule or export affordances — blocks are
// click-to-inspect only — and the run-history table likewise offers no
// export controls.
// Row: AGT-062.
// Note: "run history navigates to run detail instead of exporting" needs a
// run row to click, and the seeded stack never fires (no beat); the
// navigation code path is a Gap, while the export absence is asserted.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-062"];

test(
  specTitle(ROWS, "calendar blocks are click-only; no drag or export affordances"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag62"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const name = "AGT62 Main Definition";

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT62 Project ${tag}`,
        parityProjectIdentifier("AG62")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt62-main-${workspaceSlug}`,
        name,
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
      if (!rows.some((row) => row.scheduler === definition.id)) {
        await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
          scheduler: definition.id,
          project: created.id,
          dtstart: new Date(Date.now() - 30 * 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        });
      }
      return created;
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_name === name
      );
      if (found === undefined) throw new Error("[parity] expected the AGT62 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the firing calendar", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      await driver.schedulerCalendarStep("next");
    });

    await test.step("week blocks are click-to-inspect with no drag or export", async () => {
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).not.toEqual([]);
      expect(await driver.schedulerCalendarAnyDraggable()).toBe(false);
      expect(await driver.schedulerCalendarExportControls()).toEqual([]);
      await driver.schedulerCalendarClickBlock(name);
      expect(await driver.schedulerDrawerOpen()).toBe(true);
      await driver.schedulerDrawerClose();
      expect(await driver.schedulerDrawerOpen()).toBe(false);
    });

    await test.step("month blocks likewise offer no drag or export", async () => {
      await driver.schedulerCalendarSetView("month");
      await expect.poll(() => driver.schedulerCalendarMonthBlocks(), { timeout: 30_000 }).not.toEqual([]);
      expect(await driver.schedulerCalendarAnyDraggable()).toBe(false);
      expect(await driver.schedulerCalendarExportControls()).toEqual([]);
    });

    await test.step("the run-history table offers no export controls", async () => {
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      expect(await driver.schedulerRunsExportControls()).toEqual([]);
    });
  }
);
