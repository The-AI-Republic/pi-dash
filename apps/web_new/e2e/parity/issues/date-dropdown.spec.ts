// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): single-date dropdown — portal calendar
// popup with month/year caption, pick persists, start ≤ due disables
// out-of-range days in both directions, hover clear resets to null.
// Rows: ISS-214 (date dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverIssue,
  serverPatchIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

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
  specTitle(["ISS-214"], "date dropdown picks, constrains and clears start and due dates"),
  { tag: specTags(["ISS-214"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 dates ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // Start/Due rows render on every project, but the scenario still owns
    // its project so a sibling reseed cannot take the issue with it.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("empty issues offer Add start/due date placeholders", async () => {
        await expect
          .poll(() => driver.propertyValueText("Start date"), { timeout: 15_000 })
          .toContain("Add start date");
        await expect.poll(() => driver.propertyValueText("Due date"), { timeout: 15_000 }).toContain("Add due date");
      });

      await test.step("the calendar opens portalled on the current month", async () => {
        await driver.datePickerOpen("Start date");
        expect(await driver.datePickerVisible()).toBe(true);
        expect(await driver.datePickerPortalAttached("Start date")).toBe(true);
        const { year, month } = currentYearMonth();
        await expect
          .poll(() => driver.datePickerVisibleMonth(), { timeout: 15_000 })
          .toEqual({
            month: expect.stringContaining(MONTH_ABBREV[month] as string),
            year: String(year),
          });
        await driver.pickerPressEscape();
        await expect.poll(() => driver.datePickerVisible(), { timeout: 10_000 }).toBe(false);
      });

      await test.step("picking a start day persists and renders", async () => {
        await driver.datePickerOpen("Start date");
        await driver.datePickerPickDay(15);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).start_date, {
            timeout: 15_000,
          })
          .toBe(isoDay(15));
        await expect.poll(() => driver.propertyValueText("Start date"), { timeout: 15_000 }).toContain(displayDay(15));
      });

      await test.step("start cannot pass due and due cannot precede start", async () => {
        // Pin both ends through the API (mid-month days exist in every
        // month), then read the disabled days out of each calendar.
        await serverPatchIssue(
          seed.workspaceSlug,
          projectId,
          issue.id,
          { start_date: isoDay(10), target_date: isoDay(20) },
          session
        );
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await driver.datePickerOpen("Start date");
        expect(await driver.datePickerDayDisabled(21)).toBe(true);
        expect(await driver.datePickerDayDisabled(9)).toBe(false);
        await driver.pickerPressEscape();
        await expect.poll(() => driver.datePickerVisible(), { timeout: 10_000 }).toBe(false);
        await driver.datePickerOpen("Due date");
        expect(await driver.datePickerDayDisabled(9)).toBe(true);
        expect(await driver.datePickerDayDisabled(21)).toBe(false);
        await driver.pickerPressEscape();
        await expect.poll(() => driver.datePickerVisible(), { timeout: 10_000 }).toBe(false);
      });

      await test.step("clearing a date resets the row and the server", async () => {
        await driver.datePickerClear("Start date");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).start_date, {
            timeout: 15_000,
          })
          .toBe(null);
        await expect
          .poll(() => driver.propertyValueText("Start date"), { timeout: 15_000 })
          .toContain("Add start date");
        await driver.datePickerClear("Due date");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).target_date, {
            timeout: 15_000,
          })
          .toBe(null);
        await expect.poll(() => driver.propertyValueText("Due date"), { timeout: 15_000 }).toContain("Add due date");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
