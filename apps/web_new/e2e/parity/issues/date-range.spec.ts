// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): date-range dropdown — the issues-list
// merged-dates cell renders one smart label for the start+due pair and
// repicks either end; its clear control drops one end to the
// concurrent-PATCH race (bug:NEWFRONT-150, observed behavior); the
// cycle-create form renders the split from/to pair with today as the
// minimum day. Rows: ISS-215 (date-range dropdown).
//
// Observed behavior notes (inventory row carries them at update time):
// the merged cell only renders once the issue has BOTH dates and both
// display properties are on; the apply/cancel/bothRequired props are
// declared but never read, and no caller passes renderInPortal on these
// two surfaces (portal callers live under sub-issues and rich filters).
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCycleDetail,
  serverDeleteCycle,
  serverIssue,
  serverPatchIssue,
  serverProjectCycles,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const MONTH_ABBREV = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function currentYearMonth(): { year: number; month: number; days: number; today: number } {
  const now = new Date();
  return {
    year: now.getFullYear(),
    month: now.getMonth(),
    days: new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate(),
    today: now.getDate(),
  };
}

function isoDay(day: number): string {
  const { year, month } = currentYearMonth();
  return isoDayOf(year, month, day);
}

function isoDayOf(year: number, month0: number, day: number): string {
  return `${year}-${String(month0 + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

const MONTH_FULL = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/**
 * Click a range day and wait for the merged cell to show it. The cell
 * updates optimistically on selection (no network), so a missing update
 * means the loaded dev server swallowed the click — the calendar stays
 * open, so one more click recovers without resetting the range.
 */
async function pickRangeDay(driver: ParityDriver, day: number, issueName: string, cellSnippet: string): Promise<void> {
  await driver.rangeCalendarPickDay(day);
  const settled = await expect
    .poll(() => driver.rangeMergedCellText(issueName), { timeout: 10_000 })
    .toContain(cellSnippet)
    .then(
      () => true,
      () => false
    );
  if (!settled) {
    await driver.rangeCalendarPickDay(day);
    await expect.poll(() => driver.rangeMergedCellText(issueName), { timeout: 15_000 }).toContain(cellSnippet);
  }
}

/**
 * Pick a range day until the server converges. The app sends each pick as
 * PATCHes with no retry, so a throttled PATCH (HTTP 429 on the shared
 * scratch stack) leaves the optimistic cell ahead of the server; reloading
 * resets the cell to the server state and re-picking re-sends the update —
 * the same recovery a user performs, not an assertion retry.
 */
async function pickUntilServerSettles(
  driver: ParityDriver,
  day: number,
  issueName: string,
  cellSnippet: string,
  reopen: () => Promise<void>,
  readServer: () => Promise<string | null>,
  expected: string
): Promise<void> {
  for (let attempt = 1; attempt <= 3; attempt++) {
    if (attempt > 1) await reopen();
    await pickRangeDay(driver, day, issueName, cellSnippet);
    const settled = await expect
      .poll(readServer, { timeout: 25_000 })
      .toBe(expected)
      .then(
        () => true,
        () => false
      );
    if (settled) return;
  }
  await expect.poll(readServer, { timeout: 25_000 }).toBe(expected);
}

/**
 * Reload the project list and settle on the issue row. The list loads
 * its rows after the shell, and a stalled load leaves a blank page — so
 * settle briefly and reload once more before the hard wait.
 */
async function reloadAndSettleOnRow(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  issueName: string
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    await driver.openProjectIssues(workspaceSlug, projectId);
    const settled = await expect
      .poll(async () => (await driver.visibleIssueNames()).some((n) => n === issueName), {
        timeout: 30_000,
      })
      .toBe(true)
      .then(
        () => true,
        () => false
      );
    if (settled) return;
  }
  await expect
    .poll(async () => (await driver.visibleIssueNames()).some((n) => n === issueName), { timeout: 60_000 })
    .toBe(true);
}

test(
  specTitle(["ISS-215"], "merged list cell shows one smart label and repicks either end"),
  { tag: specTags(["ISS-215"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 range ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // Pin both ends through the API (mid-month days exist in every month)
    // so the row renders the merged cell instead of the split pickers.
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      issue.id,
      { start_date: isoDay(10), target_date: isoDay(20) },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      const { month } = currentYearMonth();
      const settle = (): Promise<void> => reloadAndSettleOnRow(driver, seed.workspaceSlug, projectId, issue.name);
      await test.step("the merged cell renders one smart label for the pair", async () => {
        // Settle on the row first: the list loads its rows after the shell.
        await settle();
        await expect.poll(() => driver.rangeMergedCellText(issue.name), { timeout: 30_000 }).toContain("10 - 20");
        const cell = await driver.rangeMergedCellText(issue.name);
        expect(cell).toContain(MONTH_ABBREV[month] as string);
      });

      await test.step("repicking either end persists it and leaves the other", async () => {
        // Observed adjust-one-end semantics: clicking inside/after the
        // range moves the to-end, clicking before the from-end moves from.
        await driver.rangeMergedCellOpen(issue.name);
        expect(await driver.rangeCalendarVisible()).toBe(true);
        const reopen = async (): Promise<void> => {
          await settle();
          await driver.rangeMergedCellOpen(issue.name);
        };
        await pickUntilServerSettles(
          driver,
          11,
          issue.name,
          "10 - 11",
          reopen,
          async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).target_date,
          isoDay(11)
        );
        await pickUntilServerSettles(
          driver,
          5,
          issue.name,
          "05",
          reopen,
          async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).start_date,
          isoDay(5)
        );
        // The from-click re-sends the current to-end, so it stays put.
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).target_date).toBe(isoDay(11));
        const cell = await driver.rangeMergedCellText(issue.name);
        expect(cell).toContain("05");
        expect(cell).toContain("11");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-215"], "bug:NEWFRONT-150 merged-cell clear drops one end to the concurrent-PATCH race"),
  { tag: specTags(["ISS-215"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 clearbug ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      issue.id,
      { start_date: isoDay(10), target_date: isoDay(20) },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await reloadAndSettleOnRow(driver, seed.workspaceSlug, projectId, issue.name);
      await expect.poll(() => driver.rangeMergedCellText(issue.name), { timeout: 30_000 }).toContain("10 - 20");
      // bug:NEWFRONT-150 — the clear control fires two concurrent
      // single-field PATCHes and the server's whole-row write drops one
      // (both 204, exactly one survives), so one click clears exactly one
      // end; which end survives varies. Intended: both ends null. If both
      // PATCHes are throttled (neither end moves), reload and click once
      // more; a partial clear needs no retry because the observation is the
      // single cleared end.
      for (let round = 1; round <= 2; round++) {
        if (round > 1) await reloadAndSettleOnRow(driver, seed.workspaceSlug, projectId, issue.name);
        await driver.rangeMergedCellClear(issue.name);
        const moved = await expect
          .poll(
            async () => {
              const current = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
              return current.start_date === null || current.target_date === null;
            },
            { timeout: 20_000 }
          )
          .toBe(true)
          .then(
            () => true,
            () => false
          );
        if (moved) break;
      }
      // The race resolves to exactly one cleared end (bug:NEWFRONT-150);
      // require that state to hold across a re-read so a straggler second
      // PATCH cannot slip in between the read and the assertion.
      const nulledEnds = async (): Promise<number> => {
        const current = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        return [current.start_date, current.target_date].filter((d) => d === null).length;
      };
      await expect
        .poll(
          async () => {
            if ((await nulledEnds()) !== 1) return 0;
            await new Promise((resolve) => setTimeout(resolve, 1000));
            return nulledEnds();
          },
          { timeout: 30_000 }
        )
        .toBe(1);
      const after = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
      expect([after.start_date, after.target_date].filter((d) => d === null)).toHaveLength(1);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-215"], "cycle form shows the split pair with today as the minimum day"),
  { tag: specTags(["ISS-215"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 split ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { cycleView: true },
      session
    );
    const cycleName = `${tag} cycle`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);

      await test.step("the split trigger shows the from/to placeholders", async () => {
        await expect
          .poll(() => driver.cycleFormRangePlaceholders(), { timeout: 15_000 })
          .toEqual({
            from: "Start date",
            to: "End date",
          });
      });

      const { days, today, month, year } = currentYearMonth();
      // A same-day double pick resets the range instead of completing it,
      // so the to-day always differs: next month when today is month-end.
      const spansMonths = today >= days;
      const fromDay = today;
      const toDay = spansMonths ? 3 : today + 1;
      const nextMonth = (month + 1) % 12;
      const nextYear = month === 11 ? year + 1 : year;
      const fromIso = isoDay(fromDay);
      const toIso = spansMonths ? isoDayOf(nextYear, nextMonth, toDay) : isoDay(toDay);
      await test.step("past days are disabled and future days pickable", async () => {
        await driver.cycleFormRangeOpen();
        expect(await driver.rangeCalendarVisible()).toBe(true);
        if (today > 1) expect(await driver.rangeCalendarDayDisabled(today - 1)).toBe(true);
        expect(await driver.rangeCalendarDayDisabled(fromDay)).toBe(false);
        await driver.rangeCalendarPickDay(fromDay);
        if (spansMonths) {
          if (month === 11) await driver.rangeCalendarSelectYear(String(nextYear));
          await driver.rangeCalendarSelectMonth(MONTH_FULL[nextMonth] as string);
        }
        await driver.rangeCalendarPickDay(toDay);
        await driver.pickerPressEscape();
        await expect.poll(() => driver.rangeCalendarVisible(), { timeout: 10_000 }).toBe(false);
      });

      await test.step("submitting persists the picked range on the cycle", async () => {
        await driver.cycleFormFillName(cycleName);
        await driver.cycleFormSubmit();
        let cycleId = "";
        await expect
          .poll(
            async () => {
              const found = (await serverProjectCycles(seed.workspaceSlug, projectId, session)).find(
                (c) => c.name === cycleName
              );
              if (found !== undefined) cycleId = found.id;
              return found?.id;
            },
            { timeout: 15_000 }
          )
          .not.toBe(undefined);
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, cycleId, session);
        // Cycle datetimes carry a midnight time component ("T00:00:01Z"),
        // so the assertions compare the date part only.
        expect(detail.startDate?.slice(0, 10)).toBe(fromIso);
        expect(detail.endDate?.slice(0, 10)).toBe(toIso);
      });
    } finally {
      const doomed = (await serverProjectCycles(seed.workspaceSlug, projectId, session)).find(
        (c) => c.name === cycleName
      );
      if (doomed !== undefined)
        await serverDeleteCycle(seed.workspaceSlug, projectId, doomed.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
