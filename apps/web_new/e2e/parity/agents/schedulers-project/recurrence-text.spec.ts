// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): recurrence rules render as plain-language
// sentences in the install list, the install detail and the calendar drawer;
// a blank rule means a single firing; the RRULE field keeps its humanizer
// line live, echoing invalid input raw; the stored rule remains the tooltip
// fallback.
// Row: AGT-020.
// Note: the list/detail fallback for a blank rule reads "Once at dtstart"
// (a literal "dtstart", unlike the dialog's "Fires once at the start
// time."). It still means a single firing, so the row holds; the area spec
// may prefer the dialog's sentence for the new app.
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

const ROWS = ["AGT-020"];

test(
  specTitle(ROWS, "recurrence renders as sentences; blank means once; humanizer echoes invalid raw"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag20"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const sentences: Record<string, string> = {
      [`agt20-daily-${workspaceSlug}`]: "every day",
      [`agt20-fortnight-${workspaceSlug}`]: "every 2 weeks on Monday, Friday",
      [`agt20-once-${workspaceSlug}`]: "Once at dtstart",
    };

    const project = await test.step("owner prepares installs with three rules", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT20 Project ${tag}`,
        parityProjectIdentifier("AG20")
      );
      const dtstart = new Date(Date.now() + 24 * 3600_000).toISOString();
      const rules: Record<string, string> = {
        [`agt20-daily-${workspaceSlug}`]: "FREQ=DAILY",
        [`agt20-fortnight-${workspaceSlug}`]: "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR",
        [`agt20-once-${workspaceSlug}`]: "",
      };
      for (const [slug, rrule] of Object.entries(rules)) {
        const definition = await ensureScheduler(workspaceSlug, ownerSession, {
          slug,
          name: `AGT20 ${slug.split("-")[1]} definition`,
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

    await test.step("owner signs in and opens the install list", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
    });

    await test.step("list rows read as sentences with the stored rule behind", async () => {
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual(server.map((row) => row.scheduler_slug).sort());
      const rows = await driver.schedulerProjectRows();
      for (const row of rows) {
        expect(row.schedule).toBe(sentences[row.handle] ?? "");
        const stored = server.find((candidate) => candidate.scheduler_slug === row.handle)?.rrule ?? "";
        expect(row.scheduleTitle).toBe(stored);
      }
    });

    await test.step("the install detail reads the same sentence with the same fallback", async () => {
      const target = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === `agt20-fortnight-${workspaceSlug}`
      );
      expect(target).toBeDefined();
      if (target === undefined) throw new Error("[parity] expected the fortnightly install.");
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, target.id);
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Schedule")?.value).toBe("every 2 weeks on Monday, Friday");
      expect(await driver.schedulerBindingScheduleTitle()).toBe("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR");
    });

    await test.step("the calendar drawer reads the same sentence for a future firing", async () => {
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      // Step into next week: a fully future window, whatever today is. The
      // daily install always fires there (the fortnightly cadence phases
      // from its anchor, so its weeks vary).
      await driver.schedulerCalendarStep("next");
      const name = `AGT20 ${`agt20-daily-${workspaceSlug}`.split("-")[1]} definition`;
      await expect
        .poll(async () => (await driver.schedulerCalendarWeekBlocks()).filter((row) => row.name === name), {
          timeout: 30_000,
        })
        .not.toEqual([]);
      await driver.schedulerCalendarClickBlock(name);
      const rows = await driver.schedulerDrawerRows();
      expect(rows.find((row) => row.label === "Recurrence")?.value).toBe("every day");
      await driver.schedulerDrawerClose();
    });

    await test.step("the dialog humanizer tracks edits with invalid-input guidance", async () => {
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerProjectOpenEdit(`agt20-daily-${workspaceSlug}`);
      expect(await driver.schedulerProjectEditHumanizer()).toBe("every day");
      await driver.schedulerProjectEditFill({ rrule: "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR" });
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("every 2 weeks on Monday, Friday");
      await driver.schedulerProjectEditFill({ rrule: "" });
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("Fires once at the start time.");
      // An invalid rule echoes raw: the humanizer returns its input on parse
      // error, so the "Invalid RRULE" fallback string never renders.
      await driver.schedulerProjectEditFill({ rrule: "FREQ=NOPE" });
      await expect.poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 }).toBe("FREQ=NOPE");
      await driver.schedulerCloseProjectEdit();
    });
  }
);
