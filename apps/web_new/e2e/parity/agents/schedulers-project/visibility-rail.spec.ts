// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): the firing calendar's visibility rail
// lists installed schedulers with per-scheduler checkboxes plus show/hide
// all; toggling filters the grid client-side; the choice persists per
// project across reloads and is visible to other tabs on load (open tabs do
// not follow each other live); the rail hides on narrow screens; a project
// with no installs shows the empty rail.
// Row: AGT-016.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-016"];

test(
  specTitle(ROWS, "visibility rail filters per scheduler with per-project local persistence"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag16"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const alphaHandle = `agt16-alpha-${workspaceSlug}`;
    const betaHandle = `agt16-beta-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with two installs", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT16 Project ${tag}`,
        parityProjectIdentifier("AG16")
      );
      const dtstart = new Date(Date.now() - 30 * 24 * 3600_000).toISOString();
      for (const [slug, name, rrule] of [
        [alphaHandle, "AGT16 Alpha Definition", "FREQ=DAILY"],
        [betaHandle, "AGT16 Beta Definition", "FREQ=DAILY"],
      ]) {
        const definition = await ensureScheduler(workspaceSlug, ownerSession, {
          slug,
          name,
          prompt: "Audit this project nightly.",
          is_enabled: true,
        });
        const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
        if (!rows.some((row) => row.scheduler === definition.id)) {
          await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
            scheduler: definition.id,
            project: created.id,
            dtstart,
            rrule,
          });
        }
      }
      return created;
    });

    await test.step("owner signs in and opens the firing calendar", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      // Step into next week: a fully future window, whatever today is.
      await driver.schedulerCalendarStep("next");
    });

    await test.step("the rail lists installed schedulers, all visible", async () => {
      expect(await driver.schedulerRailVisible()).toBe(true);
      const server = await serverSchedulers(workspaceSlug, ownerSession);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      const installedIds = new Set(bindings.map((row) => row.scheduler));
      const expected = server.filter((row) => installedIds.has(row.id)).map((row) => row.name);
      expect(expected).toContain("AGT16 Alpha Definition");
      expect(expected).toContain("AGT16 Beta Definition");
      const rows = await driver.schedulerRailRows();
      expect(rows).toEqual(expected.map((name) => ({ name, checked: true })));
    });

    await test.step("toggling one scheduler filters the grid", async () => {
      await driver.schedulerRailToggle("AGT16 Alpha Definition");
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Alpha Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(false);
      await expect
        .poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ name: "AGT16 Beta Definition" })]));
      const blocks = await driver.schedulerCalendarWeekBlocks();
      expect(blocks.length).toBeGreaterThan(0);
      expect(blocks.every((row) => row.name === "AGT16 Beta Definition")).toBe(true);
      await driver.schedulerRailToggle("AGT16 Alpha Definition");
      await expect
        .poll(async () => new Set((await driver.schedulerCalendarWeekBlocks()).map((row) => row.name)), {
          timeout: 30_000,
        })
        .toEqual(new Set(["AGT16 Alpha Definition", "AGT16 Beta Definition"]));
    });

    await test.step("hide all and show all bracket the grid", async () => {
      await driver.schedulerRailHideAll();
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toEqual([]);
      await driver.schedulerRailShowAll();
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(14);
    });

    await test.step("the choice persists per project across reloads", async () => {
      await driver.schedulerRailToggle("AGT16 Beta Definition");
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Beta Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(false);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      await expect
        .poll(() => driver.schedulerRailRows(), { timeout: 30_000 })
        .toEqual(
          expect.arrayContaining([
            expect.objectContaining({ name: "AGT16 Alpha Definition", checked: true }),
            expect.objectContaining({ name: "AGT16 Beta Definition", checked: false }),
          ])
        );
      // A second project with the same installs keeps its own choice.
      const other = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT16 Other ${tag}`,
        parityProjectIdentifier("AG1O")
      );
      const dtstart = new Date(Date.now() - 30 * 24 * 3600_000).toISOString();
      for (const row of await serverSchedulers(workspaceSlug, ownerSession)) {
        if (row.slug !== alphaHandle && row.slug !== betaHandle) continue;
        await serverCreateBinding(workspaceSlug, other.id, ownerSession, {
          scheduler: row.id,
          project: other.id,
          dtstart,
          rrule: "FREQ=DAILY",
        }).catch(() => undefined);
      }
      await driver.schedulerOpenProjectCalendar(workspaceSlug, other.id);
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Beta Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(true);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Beta Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(false);
      await driver.schedulerRailShowAll();
    });

    await test.step("the rail hides on narrow screens", async () => {
      expect(await driver.schedulerRailNarrowHidden()).toBe(true);
      expect(await driver.schedulerRailVisible()).toBe(true);
    });

    await test.step("the choice persists across tabs", async () => {
      await driver.schedulerRailShowAll();
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Alpha Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(true);
      // A second tab loads with the choice applied (load-time state; open
      // tabs do not follow each other live).
      expect(await driver.schedulerRailCrossTabPersists(workspaceSlug, project.id, "AGT16 Alpha Definition")).toBe(
        true
      );
      await expect
        .poll(
          async () => (await driver.schedulerRailRows()).find((row) => row.name === "AGT16 Alpha Definition")?.checked,
          { timeout: 30_000 }
        )
        .toBe(false);
      await driver.schedulerRailShowAll();
    });

    await test.step("a project with no installs shows the empty rail", async () => {
      const bare = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT16 Bare ${tag}`,
        parityProjectIdentifier("AG1B")
      );
      await driver.schedulerOpenProjectCalendar(workspaceSlug, bare.id);
      expect(await driver.schedulerCalendarEmptyVisible()).toBe(true);
      expect(await driver.schedulerRailVisible()).toBe(true);
      expect(await driver.schedulerRailRows()).toEqual([]);
    });
  }
);
