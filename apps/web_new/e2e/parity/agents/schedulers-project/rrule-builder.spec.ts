// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-892): the Google-Calendar-style recurrence
// builder (main#561, restated in §14 of the agents spec). One widget over
// the stored bare rule string: repeat count plus unit, Sunday-first weekday
// chips that never go empty, a monthly day-or-nth-weekday choice defaulting
// from the start, and an Ends group (never, on-date ending at the end of
// that day UTC, after-N with a near-term prefill). The raw rule textarea
// stays under an advanced disclosure; serialization is the API validator's
// bare form, and opening reconstructs state from the stored rule. A stored
// rule the widget cannot express opens raw and submits byte-identical
// unless actually edited.
//
// Oracle leg (NEWFRONT-192): main#561 landed the same builder in the old
// app, so both legs drive the widget and only the known divergences
// branch per step: the old unit order lists does-not-repeat last, its
// on-date prefill is today-plus-90-days (not start-plus-a-year), its
// after-N prefill is 13 (not 10), and it switches the monthly mode
// without day or ordinal controls. The sentence wording also differs
// for a monthly day: the oracle prints rrule's `toText()` ("every month
// on the 15th", no trailing full stop), while the builder's own sentence
// says "every month on day 15" (NEWFRONT-899 dropped the invented stop).
// Row: AGT-065.
import { test, expect } from "../../fixtures";
import {
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindingDetail,
  serverBindings,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import type { ParityDriver } from "../../drivers/parity-driver";

const ROWS = ["AGT-065"];

/** The install's rrule as the server currently holds it. */
async function storedRule(workspaceSlug: string, projectId: string, handle: string, session: string): Promise<string> {
  const row = (await serverBindings(workspaceSlug, projectId, session)).find(
    (candidate) => candidate.scheduler_slug === handle
  );
  if (row === undefined) throw new Error(`[parity] expected the AGT65 install ${handle}.`);
  return (await serverBindingDetail(workspaceSlug, projectId, row.id, session)).rrule;
}

/** A start on a known weekday and day of month: 2026-11-09 is a Monday. */
const START = "2026-11-09T09:30";

async function openEdit(driver: ParityDriver, workspaceSlug: string, projectId: string, handle: string): Promise<void> {
  await driver.schedulerOpenProjectList(workspaceSlug, projectId);
  await driver.schedulerProjectOpenEdit(handle);
}

test(
  specTitle(ROWS, "recurrence builder covers units, chips, monthly, ends, raw fallback and serialization"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt65"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt65-builder-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT65 Project ${tag}`,
        parityProjectIdentifier("AG65")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT65 Builder Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        dtstart: new Date(START).toISOString(),
        tzid: "UTC",
        rrule: "FREQ=DAILY",
      });
      return created;
    });

    await test.step("owner signs in and opens the edit dialog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await openEdit(driver, workspaceSlug, project.id, handle);
    });

    await test.step("opening reconstructs widget state from the stored rule", async () => {
      const facts = await driver.schedulerRecurrenceFacts();
      expect(facts.visible).toBe(true);
      expect(facts.unit).toBe("day");
      expect(facts.interval).toBe("1");
      expect(facts.ends).toBe("never");
      expect(facts.rawVisible).toBe(false);
      expect(facts.advancedVisible).toBe(true);
      expect(facts.sentence).toBe("every day");
      if (driver.target !== "web_new") {
        // The oracle unmounts its raw field while closed, so the steps
        // below that poll the serialized rule need it open from here on.
        await expect(driver.schedulerRecurrenceAdvanced()).resolves.toBe(true);
      }
    });

    await test.step("repeat count and unit serialize a bare frequency", async () => {
      // The new builder offers Hour ahead of Day (DIFF-2, new-system-only);
      // the old widget has no sub-daily cadence and lists does-not-repeat
      // last instead of first.
      const units = (await driver.schedulerRecurrenceFacts()).unitOptions;
      expect(units).toEqual(
        driver.target === "web_new"
          ? ["none", "hour", "day", "week", "month", "year"]
          : ["day", "week", "month", "year", "none"]
      );
      expect(units.filter((unit) => /hour|minute|second/i.test(unit))).toEqual(
        driver.target === "web_new" ? ["hour"] : []
      );
      await driver.schedulerRecurrenceAct({ kind: "interval", value: 3 });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=DAILY;INTERVAL=3");
      // A typed zero or negative clamps to one, which serializes without
      // INTERVAL rather than emitting `INTERVAL=0`.
      await driver.schedulerRecurrenceAct({ kind: "interval", value: 0 });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=DAILY");
      await driver.schedulerRecurrenceAct({ kind: "interval", value: -4 });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=DAILY");
      await driver.schedulerRecurrenceAct({ kind: "interval", value: 1 });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=DAILY");
      await driver.schedulerRecurrenceAct({ kind: "unit", value: "none" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("");
      expect((await driver.schedulerRecurrenceFacts()).interval).toBeNull();
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).sentence)
        .toBe("Fires once at the start time.");
    });

    await test.step("weekly chips default from the start and never go empty", async () => {
      await driver.schedulerRecurrenceAct({ kind: "unit", value: "week" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=WEEKLY;BYDAY=MO");
      let facts = await driver.schedulerRecurrenceFacts();
      expect(facts.weekdays.map((day) => day.name)).toEqual([
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
      ]);
      expect(facts.weekdays.filter((day) => day.pressed).map((day) => day.name)).toEqual(["Monday"]);
      // Untick the only selected chip: the set must not empty.
      await driver.schedulerRecurrenceAct({ kind: "weekday", value: "Monday" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=WEEKLY;BYDAY=MO");
      expect((await driver.schedulerRecurrenceFacts()).weekdays.filter((day) => day.pressed)).toHaveLength(1);
      await driver.schedulerRecurrenceAct({ kind: "weekday", value: "Friday" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=WEEKLY;BYDAY=MO,FR");
      expect((await driver.schedulerRecurrenceFacts()).sentence).toBe("every week on Monday, Friday");
    });

    await test.step("monthly defaults from the start day and switches to nth weekday", async () => {
      await driver.schedulerRecurrenceAct({ kind: "unit", value: "month" });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=MONTHLY;BYMONTHDAY=9");
      let facts = await driver.schedulerRecurrenceFacts();
      expect(facts.monthlyPattern).toBe("day");
      expect(facts.dayOfMonth).toBe("9");
      await driver.schedulerRecurrenceAct({ kind: "monthly", value: "weekday" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=MONTHLY;BYDAY=2MO");
      facts = await driver.schedulerRecurrenceFacts();
      expect(facts.ordinal).toBe("2");
      expect(facts.weekdayOfMonth).toBe("MO");
      // The old widget switches the monthly mode but offers no day or
      // ordinal controls, so those edits stay target-only.
      if (driver.target !== "web_new") return;
      await driver.schedulerRecurrenceAct({ kind: "ordinal", value: "-1" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=MONTHLY;BYDAY=-1MO");
      await driver.schedulerRecurrenceAct({ kind: "monthly", value: "day" });
      await driver.schedulerRecurrenceAct({ kind: "day", value: 15 });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=MONTHLY;BYMONTHDAY=15");
    });

    await test.step("Ends offers never, on-date at end-of-day UTC and after-N with a prefill", async () => {
      await driver.schedulerRecurrenceAct({ kind: "unit", value: "week" });
      await driver.schedulerRecurrenceAct({ kind: "ends", value: "on" });
      let facts = await driver.schedulerRecurrenceFacts();
      if (driver.target === "web_new") {
        expect(facts.endDate).toBe("2027-11-09");
      } else {
        // The old widget prefills the on-date with today plus 90 days.
        const prefill = new Date();
        prefill.setUTCDate(prefill.getUTCDate() + 90);
        expect(facts.endDate).toBe(prefill.toISOString().slice(0, 10));
      }
      await driver.schedulerRecurrenceAct({ kind: "endDate", value: "2027-01-05" });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=WEEKLY;BYDAY=MO;UNTIL=20270105T235959Z");
      await driver.schedulerRecurrenceAct({ kind: "ends", value: "after" });
      facts = await driver.schedulerRecurrenceFacts();
      expect(facts.occurrences).toBe(driver.target === "web_new" ? "10" : "13");
      await driver.schedulerRecurrenceAct({ kind: "occurrences", value: 4 });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=WEEKLY;BYDAY=MO;COUNT=4");
      await driver.schedulerRecurrenceAct({ kind: "ends", value: "never" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=WEEKLY;BYDAY=MO");
      expect((await driver.schedulerRecurrenceFacts()).endDate).toBeNull();
      expect((await driver.schedulerRecurrenceFacts()).occurrences).toBeNull();
    });

    await test.step("the raw textarea sits under the advanced disclosure and re-syncs the widget", async () => {
      if (driver.target !== "web_new") {
        // The builder steps above left the oracle's raw field open, so
        // prove the disclosure closes before proving it opens.
        await expect(driver.schedulerRecurrenceAdvanced()).resolves.toBe(false);
      }
      expect((await driver.schedulerRecurrenceFacts()).rawVisible).toBe(false);
      await expect(driver.schedulerRecurrenceAdvanced()).resolves.toBe(true);
      expect((await driver.schedulerRecurrenceFacts()).rawVisible).toBe(true);
      await driver.schedulerRecurrenceRaw("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR");
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR");
      const facts = await driver.schedulerRecurrenceFacts();
      expect(facts.unit).toBe("week");
      expect(facts.interval).toBe("2");
      expect(facts.sentence).toBe("every 2 weeks on Monday, Friday");
    });

    await test.step("saving persists exactly what the builder serialized", async () => {
      await driver.schedulerProjectEditSubmit();
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      // Both legs typed the same raw rule above, so both prove the save
      // round-trips the editor's current text.
      expect(await storedRule(workspaceSlug, project.id, handle, ownerSession)).toBe(
        "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR"
      );
    });

    await test.step("reopening reconstructs the builder from the saved rule", async () => {
      await openEdit(driver, workspaceSlug, project.id, handle);
      // The saved rule is not the builder's default: unit, interval and the
      // pressed weekday chips all have to come back from it.
      const facts = await driver.schedulerRecurrenceFacts();
      expect(facts.visible).toBe(true);
      expect(facts.unit).toBe("week");
      expect(facts.interval).toBe("2");
      expect(facts.weekdays.filter((day) => day.pressed).map((day) => day.name)).toEqual(["Monday", "Friday"]);
      expect(facts.advancedVisible).toBe(true);
      expect(facts.rawVisible).toBe(false);
      await driver.schedulerCloseProjectEdit();
    });

    await test.step("an inexpressible rule opens raw and submits byte-identical unless edited", async () => {
      // BYHOUR is a valid RFC 5545 rule the repeat controls cannot show;
      // the server stores it verbatim (it only strips an `RRULE:` prefix).
      const stored = "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9";
      await openEdit(driver, workspaceSlug, project.id, handle);
      await driver.schedulerRecurrenceRaw(stored);
      await driver.schedulerProjectEditSubmit();
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      expect(await storedRule(workspaceSlug, project.id, handle, ownerSession)).toBe(stored);

      await openEdit(driver, workspaceSlug, project.id, handle);
      // An inexpressible rule forces raw mode with no disclosure on both
      // legs; the stored bytes show verbatim.
      const facts = await driver.schedulerRecurrenceFacts();
      expect(facts.rawVisible).toBe(true);
      expect(facts.advancedVisible).toBe(false);
      expect(facts.rawValue).toBe(stored);
      // Untouched: the stored bytes must round-trip through the editor.
      await driver.schedulerProjectEditSubmit();
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      expect(await storedRule(workspaceSlug, project.id, handle, ownerSession)).toBe(stored);

      await openEdit(driver, workspaceSlug, project.id, handle);
      await driver.schedulerRecurrenceRaw("FREQ=DAILY;INTERVAL=1");
      await driver.schedulerProjectEditSubmit();
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      // Edited: the written rule is saved exactly as typed.
      expect(await storedRule(workspaceSlug, project.id, handle, ownerSession)).toBe("FREQ=DAILY;INTERVAL=1");
    });

    await test.step("the live plain-language line follows every change", async () => {
      await openEdit(driver, workspaceSlug, project.id, handle);
      await driver.schedulerRecurrenceRaw("FREQ=MONTHLY;BYMONTHDAY=15");
      // The builder's own sentence says "day 15"; the oracle renders the
      // same rule through rrule's `toText()`, which says "the 15th". Both
      // legs prove the line follows the rule, so only the wording branch is
      // target-conditional.
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe(driver.target === "web_new" ? "every month on day 15" : "every month on the 15th");
      await driver.schedulerRecurrenceRaw("FREQ=WEEKLY;BYDAY=MO,FR");
      // Neither leg adds punctuation to a rendered sentence; only the blank
      // rule's translated single-fire sentence carries its own full stop.
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("every week on Monday, Friday");
      await driver.schedulerRecurrenceRaw("");
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("Fires once at the start time.");
      await driver.schedulerCloseProjectEdit();
    });
  }
);
