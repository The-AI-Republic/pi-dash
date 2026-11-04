// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): the project schedulers section has
// list/calendar tabs with the calendar as the bare-path landing; tabs stay
// unhighlighted on install-detail routes; the settings variant reuses the
// installs panel behind a project-admin gate while the project page shows
// non-admins a read-only list. New Scheduler sits in the tab row for
// project admins on every tab (PDASHOSS01-348) with no second copy in the
// list header, and an install started from the calendar shows there
// without a reload.
// Row: AGT-021.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-021"];

/** A datetime-local value a few minutes ahead of now, in the browser's zone. */
function localInputMinutesOut(minutes: number): string {
  const at = new Date(Date.now() + minutes * 60_000);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(
    at.getMinutes()
  )}`;
}

test(
  specTitle(
    ROWS,
    "section tabs default to calendar; New Scheduler in the tab row; settings variant gates on project admin"
  ),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag21"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt21-main-${workspaceSlug}`;
    const calendarHandle = `agt21-cal-${workspaceSlug}`;
    const calendarName = "AGT21 Calendar Definition";

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT21 Project ${tag}`,
        parityProjectIdentifier("AG21")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT21 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: calendarHandle,
        name: calendarName,
        prompt: "Check this project from the calendar.",
        is_enabled: true,
      });
      const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
      if (!rows.some((row) => row.scheduler === definition.id)) {
        await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
          scheduler: definition.id,
          project: created.id,
          dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        });
      }
      return created;
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT21 install.");
      return found.id;
    };

    await test.step("owner signs in; the bare section path lands on the calendar", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectSection(workspaceSlug, project.id);
      expect(await driver.schedulerCalendarView()).toBe("week");
    });

    await test.step("tabs highlight the active view and navigate", async () => {
      expect(await driver.schedulerSectionTabs()).toEqual([
        { label: "List", active: false },
        { label: "Calendar", active: true },
      ]);
      await driver.schedulerSectionOpenTab("List");
      expect(await driver.schedulerSectionTabs()).toEqual([
        { label: "List", active: true },
        { label: "Calendar", active: false },
      ]);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(handle);
      await driver.schedulerSectionOpenTab("Calendar");
      expect(await driver.schedulerSectionTabs()).toEqual([
        { label: "List", active: false },
        { label: "Calendar", active: true },
      ]);
    });

    await test.step("New Scheduler sits in the tab row on both tabs, once", async () => {
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: true, total: 1 });
      await driver.schedulerSectionOpenTab("List");
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: true, total: 1 });
      await driver.schedulerSectionOpenTab("Calendar");
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: true, total: 1 });
    });

    await test.step("an install started from the calendar shows there without a reload", async () => {
      await driver.schedulerOpenProjectInstall();
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), {
          timeout: 30_000,
        })
        .toContain(calendarHandle);
      await driver.schedulerProjectInstallSelect(calendarHandle);
      // A daily rule starting minutes from now fires inside the visible week.
      await driver.schedulerProjectInstallFillSchedule({ dtstart: localInputMinutesOut(10), rrule: "FREQ=DAILY" });
      await driver.schedulerProjectInstallSubmit();
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      expect(
        (await serverBindings(workspaceSlug, project.id, ownerSession)).some(
          (row) => row.scheduler_slug === calendarHandle
        )
      ).toBe(true);
      await expect
        .poll(async () => (await driver.schedulerCalendarWeekBlocks()).map((block) => block.name), {
          timeout: 30_000,
        })
        .toContain(calendarName);
      await driver.schedulerSectionOpenTab("List");
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(calendarHandle);
    });

    await test.step("tabs stay unhighlighted on install-detail routes", async () => {
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      expect(await driver.schedulerSectionTabs()).toEqual([
        { label: "List", active: false },
        { label: "Calendar", active: false },
      ]);
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: true, total: 1 });
    });

    await test.step("the settings variant reuses the installs panel for project admins", async () => {
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
      expect(await driver.schedulerSettingsPanelVisible()).toBe(true);
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual(server.map((row) => row.scheduler_slug).sort());
      expect(await driver.schedulerProjectNewVisible()).toBe(true);
    });

    await test.step("non-admins are refused on settings but read the project list", async () => {
      const member = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.MEMBER, "parity-ag21m");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
      expect(await driver.schedulerSettingsPanelVisible()).toBe(false);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual(server.map((row) => row.scheduler_slug).sort());
      expect(await driver.schedulerProjectNewVisible()).toBe(false);
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: false, total: 0 });
      await driver.schedulerSectionOpenTab("Calendar");
      expect(await driver.schedulerSectionNewPlacement()).toEqual({ inTabRow: false, total: 0 });
    });
  }
);
