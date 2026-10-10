// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the inline date pickers with the
// start-before-due rule, and the estimate picker's enablement gate.
// Rows: DRAFT-016, DRAFT-017. Green on apps/web first.
//
// DRAFT-017's enabled-project half is a bug: scenario (the oracle never
// renders the estimate picker on drafts); that scenario lives below once
// its linked issue exists.
import { test, expect } from "../fixtures";
import {
  serverCreateDraft,
  serverCreateEstimate,
  serverDraftRecord,
  serverPatchDraft,
  serverPatchDraftStatus,
  serverProjectEstimate,
  serverSetProjectEstimate,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs } from "./support";

const MONTH_ABBREV = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function currentYearMonth(): { year: number; month: number } {
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() };
}

function isoDay(day: number): string {
  const { year, month } = currentYearMonth();
  return `${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

function displayDay(day: number): string {
  const { year, month } = currentYearMonth();
  return `${MONTH_ABBREV[month]} ${String(day).padStart(2, "0")}, ${year}`;
}

test(
  specTitle(["DRAFT-016"], "dates change inline and persist across reloads"),
  { tag: specTags(["DRAFT-016"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d16");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D16 Dates ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, {
      name,
      project_id: projectId,
      start_date: isoDay(10),
      target_date: isoDay(20),
    });

    await test.step("valid dates render and persist", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftDatesText(name), { timeout: 60_000 }).toContain(displayDay(10));
      await driver.draftOpenStartDatePicker(name);
      await driver.datePickerPickDay(12);
      await expect.poll(() => driver.draftDatesText(name), { timeout: 30_000 }).toContain(displayDay(12));
      // Sequential edits: the start save must land before the due save goes
      // out, or the two full-payload PATCHes race and the earlier one wins.
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).startDate, {
          timeout: 30_000,
        })
        .toBe(isoDay(12));
      await driver.draftOpenDueDatePicker(name);
      await driver.datePickerPickDay(18);
      await expect.poll(() => driver.draftDatesText(name), { timeout: 30_000 }).toContain(displayDay(18));
      // The due save may still be in flight when the optimistic UI
      // already shows it; wait for the server to converge, then pin.
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).targetDate, {
          timeout: 30_000,
        })
        .toBe(isoDay(18));
      const stored = await serverDraftRecord(workspaceSlug, draft.id, owner.cookie);
      expect(stored.startDate).toBe(isoDay(12));
      expect(stored.targetDate).toBe(isoDay(18));
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftDatesText(name), { timeout: 60_000 }).toContain(displayDay(12));
      expect(await driver.draftDatesText(name)).toContain(displayDay(18));
    });

    await test.step("an inverted range is refused and the old dates stay", async () => {
      const before = await serverDraftRecord(workspaceSlug, draft.id, owner.cookie);
      const attempt = await serverPatchDraftStatus(workspaceSlug, draft.id, owner.cookie, {
        start_date: isoDay(25),
        target_date: isoDay(5),
      });
      expect(attempt.status).toBe(400);
      const after = await serverDraftRecord(workspaceSlug, draft.id, owner.cookie);
      expect(after.startDate).toBe(before.startDate);
      expect(after.targetDate).toBe(before.targetDate);
    });

    await test.step("the calendars disable out-of-range days", async () => {
      await serverPatchDraft(workspaceSlug, draft.id, owner.cookie, {
        start_date: isoDay(10),
        target_date: isoDay(20),
      });
      await draftsOpenAs(driver, harness);
      await driver.draftOpenStartDatePicker(name);
      expect(await driver.datePickerDayDisabled(21)).toBe(true);
      expect(await driver.datePickerDayDisabled(9)).toBe(false);
      await driver.pickerPressEscape();
      await driver.draftOpenDueDatePicker(name);
      expect(await driver.datePickerDayDisabled(9)).toBe(true);
      expect(await driver.datePickerDayDisabled(21)).toBe(false);
      await driver.pickerPressEscape();
    });
  }
);

test(
  specTitle(["DRAFT-017"], "projects without estimates show no estimate control"),
  { tag: specTags(["DRAFT-017"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d17");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D17 No Estimate ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the row carries no estimate control", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftEstimatePickerVisible(name)).toBe(false);
      expect(await driver.draftEstimateText(name)).toBe("");
    });
  }
);

// The drafts screen reads projects through the list payload, which never
// carries the estimate link, so the picker never renders even where
// enabled. Pinned here until NEWFRONT-300 lands the fix.
test(
  specTitle(["DRAFT-017"], "bug: enabled projects never offer the estimate picker (NEWFRONT-300)"),
  { tag: specTags(["DRAFT-017"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d17bug");
    const { owner, workspaceSlug, projectId } = harness;
    const system = await serverCreateEstimate(workspaceSlug, projectId, "Fib", ["1", "2", "3"], owner.cookie);
    await serverSetProjectEstimate(workspaceSlug, projectId, system.id, owner.cookie);
    expect(await serverProjectEstimate(workspaceSlug, projectId, owner.cookie)).toBe(system.id);
    const name = `D17 Enabled ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, {
      name,
      project_id: projectId,
      estimate_point: system.points[1]?.id,
    });
    expect(draft.estimatePoint).toBe(system.points[1]?.id);

    await test.step("the linked project still offers no control", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftEstimatePickerVisible(name)).toBe(false);
      expect(await driver.draftEstimateText(name)).toBe("");
      expect((await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).estimatePoint).toBe(system.points[1]?.id);
    });
  }
);
