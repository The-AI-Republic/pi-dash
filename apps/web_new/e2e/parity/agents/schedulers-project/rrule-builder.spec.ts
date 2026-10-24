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
// Target note: this checkout has only a plain RRULE textarea, so the
// builder-specific reads and interactions are target-only. The oracle leg
// still proves the shared halves it can express: the stored rule pre-fills
// verbatim, raw editing persists, and the live sentence tracks it. Those
// branches are documented per step.
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
      if (driver.target !== "web_new") {
        // The oracle's plain textarea still pre-fills the stored rule.
        expect((await driver.schedulerProjectEditValues()).rrule).toBe("FREQ=DAILY");
        return;
      }
      const facts = await driver.schedulerRecurrenceFacts();
      expect(facts.visible).toBe(true);
      expect(facts.unit).toBe("day");
      expect(facts.interval).toBe("1");
      expect(facts.ends).toBe("never");
      expect(facts.rawVisible).toBe(false);
      expect(facts.advancedVisible).toBe(true);
      expect(facts.sentence).toBe("every day");
    });

    await test.step("repeat count and unit serialize a bare frequency", async () => {
      if (driver.target !== "web_new") return;
      // The unit control offers no sub-daily cadence: the builder's
      // smallest repeat is a day, as §14 requires.
      const units = (await driver.schedulerRecurrenceFacts()).unitOptions;
      expect(units).toEqual(["none", "day", "week", "month", "year"]);
      expect(units.filter((unit) => /hour|minute|second/i.test(unit))).toEqual([]);
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
      if (driver.target !== "web_new") return;
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
      if (driver.target !== "web_new") return;
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
      await driver.schedulerRecurrenceAct({ kind: "ordinal", value: "-1" });
      await expect.poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue).toBe("FREQ=MONTHLY;BYDAY=-1MO");
      await driver.schedulerRecurrenceAct({ kind: "monthly", value: "day" });
      await driver.schedulerRecurrenceAct({ kind: "day", value: 15 });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=MONTHLY;BYMONTHDAY=15");
    });

    await test.step("Ends offers never, on-date at end-of-day UTC and after-N with a prefill", async () => {
      if (driver.target !== "web_new") return;
      await driver.schedulerRecurrenceAct({ kind: "unit", value: "week" });
      await driver.schedulerRecurrenceAct({ kind: "ends", value: "on" });
      let facts = await driver.schedulerRecurrenceFacts();
      expect(facts.endDate).toBe("2027-11-09");
      await driver.schedulerRecurrenceAct({ kind: "endDate", value: "2027-01-05" });
      await expect
        .poll(async () => (await driver.schedulerRecurrenceFacts()).rawValue)
        .toBe("FREQ=WEEKLY;BYDAY=MO;UNTIL=20270105T235959Z");
      await driver.schedulerRecurrenceAct({ kind: "ends", value: "after" });
      facts = await driver.schedulerRecurrenceFacts();
      expect(facts.occurrences).toBe("10");
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
      if (driver.target !== "web_new") return;
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
      // The builder steps above are target-only, so the oracle still holds
      // its untouched stored rule; both legs prove the save round-trips the
      // editor's current text.
      const expected = driver.target === "web_new" ? "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR" : "FREQ=DAILY";
      expect(await storedRule(workspaceSlug, project.id, handle, ownerSession)).toBe(expected);
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
      if (driver.target === "web_new") {
        const facts = await driver.schedulerRecurrenceFacts();
        expect(facts.rawVisible).toBe(true);
        expect(facts.advancedVisible).toBe(false);
        expect(facts.rawValue).toBe(stored);
      }
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
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("every month on day 15");
      await driver.schedulerRecurrenceRaw("FREQ=WEEKLY;BYDAY=MO,FR");
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
