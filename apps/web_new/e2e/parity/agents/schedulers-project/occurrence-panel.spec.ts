// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): selecting a future firing opens a side
// panel with its scheduler, firing time, time zone, recurrence and project
// context; the panel always links to the parent install and offers
// edit-binding to project admins only; it closes via its dismiss control
// and renders nothing with no selection.
// Row: AGT-017.
// Note: past occurrences need AgentRun rows, which the seeded stack never
// produces (no beat), so the past-run halves (run status, run link) are a
// Gap — the future halves below carry the row.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverOccurrences,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-017"];

/** Next Sunday 00:00 local through the Saturday after: the stepped-to week window. */
function nextWeekWindow(): { from: string; to: string } {
  const now = new Date();
  const sunday = new Date(now);
  sunday.setDate(now.getDate() + ((7 - now.getDay()) % 7 || 7));
  sunday.setHours(0, 0, 0, 0);
  return { from: sunday.toISOString(), to: new Date(sunday.getTime() + 7 * 24 * 3600_000 - 1).toISOString() };
}

test(
  specTitle(ROWS, "occurrence panel explains a firing and links its install"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag17"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const name = "AGT17 Main Definition";

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT17 Project ${tag}`,
        parityProjectIdentifier("AG17")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt17-main-${workspaceSlug}`,
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
          tzid: "Pacific/Auckland",
          rrule: "FREQ=DAILY",
          extra_context: "AGT17 project framing.",
        });
      }
      return created;
    });

    await test.step("owner signs in and opens the firing calendar", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      expect(await driver.schedulerDrawerOpen()).toBe(false);
      // Step into next week: a fully future window, whatever today is.
      await driver.schedulerCalendarStep("next");
    });

    await test.step("selecting a future firing explains it with install links", async () => {
      const window = nextWeekWindow();
      const api = await serverOccurrences(workspaceSlug, project.id, ownerSession, window.from, window.to);
      expect(api.occurrences.length).toBe(7);
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
      await driver.schedulerCalendarClickBlock(name);
      expect(await driver.schedulerDrawerOpen()).toBe(true);
      const heading = await driver.schedulerDrawerHeading();
      expect(heading).toEqual({ state: "Scheduled", name });
      const rows = await driver.schedulerDrawerRows();
      const valueOf = (label: string): string | undefined => rows.find((row) => row.label === label)?.value;
      // The clicked block is the week's earliest firing; its instant is one
      // of the window's occurrences.
      const when = new Date(valueOf("When") ?? "").getTime();
      expect(Number.isNaN(when)).toBe(false);
      expect(api.occurrences.some((row) => Math.abs(new Date(row.dtstart).getTime() - when) < 60_000)).toBe(true);
      expect(valueOf("Time zone")).toBe("Pacific/Auckland");
      expect(valueOf("Recurrence")).toBe("every day");
      expect(valueOf("Project context")).toBe("AGT17 project framing.");
      const links = await driver.schedulerDrawerLinks();
      expect(links).toEqual(["View scheduler →"]);
      expect(await driver.schedulerDrawerEditVisible()).toBe(true);
    });

    await test.step("the panel links to the parent install", async () => {
      await driver.schedulerDrawerViewScheduler();
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe(name);
    });

    await test.step("the panel closes via its dismiss control", async () => {
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
      await driver.schedulerCalendarClickBlock(name);
      expect(await driver.schedulerDrawerOpen()).toBe(true);
      await driver.schedulerDrawerClose();
      expect(await driver.schedulerDrawerOpen()).toBe(false);
    });

    await test.step("edit-binding opens the edit dialog for project admins", async () => {
      await driver.schedulerCalendarClickBlock(name);
      expect(await driver.schedulerDrawerEditVisible()).toBe(true);
      await driver.schedulerDrawerEdit();
      expect(await driver.schedulerDrawerOpen()).toBe(false);
      expect(await driver.schedulerProjectEditOpen()).toBe(true);
      expect((await driver.schedulerProjectEditValues()).rrule).toBe("FREQ=DAILY");
      await driver.schedulerCloseProjectEdit();
    });

    await test.step("a project member inspects without edit-binding", async () => {
      const member = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.MEMBER, "parity-ag17m");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
      await driver.schedulerCalendarClickBlock(name);
      const heading = await driver.schedulerDrawerHeading();
      expect(heading).toEqual({ state: "Scheduled", name });
      expect(await driver.schedulerDrawerEditVisible()).toBe(false);
      const links = await driver.schedulerDrawerLinks();
      expect(links).toEqual(["View scheduler →"]);
      await driver.schedulerDrawerViewScheduler();
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe(name);
      expect(header.editVisible).toBe(false);
    });
  }
);
