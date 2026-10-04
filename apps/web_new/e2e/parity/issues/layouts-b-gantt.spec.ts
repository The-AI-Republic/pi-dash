// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-118): Issues gantt / timeline layout —
// header, zoom, today, fullscreen, infinite extension, sidebar rows and
// their reorder, bar move/resize/add-block/quick-add, hover preview,
// scroll-to-block, permission gating, and loading states. Rows:
// ISS-044..ISS-059.
// Precondition: a fresh seeded stack (parity-up.sh); every scenario sets
// its own layout preferences through the API, enters the app
// pre-authenticated, and deletes the rows it creates so later scenarios
// see the seed again.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/index";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverDefaultStateId,
  serverDeleteIssue,
  serverDeleteProject,
  serverIssueDetails,
  serverIssues,
  serverPatchIssue,
  serverPatchProject,
  serverPatchProjectUserProperties,
  serverProfileStartOfWeek,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
  type FreshUser,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** An opened timeline plus the prefs it started with, for teardown restore. */
interface TimelineContext {
  user: FreshUser;
  beforeFilters: Record<string, unknown>;
  beforeProperties: Record<string, unknown>;
}

/**
 * Set a project's timeline preferences through the API, then enter the app
 * pre-authenticated and wait for the timeline to render. Manual order is
 * the default; callers override per scenario.
 */
async function openTimeline(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  filters: Record<string, unknown> = {}
): Promise<TimelineContext> {
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "gantt_chart",
      group_by: null,
      order_by: "sort_order",
      sub_group_by: null,
      show_empty_groups: true,
      ...filters,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await ganttOpenWithRetry(driver);
  return { user, beforeFilters: before.displayFilters, beforeProperties: before.displayProperties };
}

/**
 * Open the timeline, retrying once through a reload. Under host
 * contention the issues page occasionally never renders its switcher
 * inside the driver's long wait (seen 3× in 5 full-file sweeps, always
 * with sibling suites running alongside). The retry only absorbs that
 * load stall: a genuinely broken timeline still fails its second wait.
 */
async function ganttOpenWithRetry(driver: ParityDriver): Promise<void> {
  try {
    await driver.ganttOpenTimeline();
  } catch {
    await driver.boardReloadIssues();
    await driver.ganttOpenTimeline();
  }
}

/**
 * Reload the issues page, retrying once when the reload itself stalls.
 * Under host contention the reloaded page occasionally never renders its
 * switcher inside the driver's long wait; the kanban re-verify lost five
 * attempts to that single unguarded reload. The retry only absorbs the
 * stall: a genuinely wedged page still fails its second wait.
 */
async function boardReloadWithRetry(driver: ParityDriver): Promise<void> {
  try {
    await driver.boardReloadIssues();
  } catch {
    await driver.boardReloadIssues();
  }
}

/** Reload the issues page and wait for the timeline, absorbing one load stall at each step. */
async function ganttReloadWithRetry(driver: ParityDriver): Promise<void> {
  await boardReloadWithRetry(driver);
  await ganttOpenWithRetry(driver);
}

/** Restore the exact preferences an opened timeline started with. */
async function restoreTimeline(seed: ParitySeedFacts, projectId: string, ctx: TimelineContext): Promise<void> {
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
    display_filters: ctx.beforeFilters,
    display_properties: ctx.beforeProperties,
  });
}

/** Switch one timeline preference and reload so the timeline re-renders. */
async function setTimelineFilters(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  ctx: TimelineContext,
  filters: Record<string, unknown>
): Promise<void> {
  const current = await serverProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
    display_filters: { ...current.displayFilters, ...filters },
  });
  await ganttReloadWithRetry(driver);
}

/** Server-side UUID of an issue looked up by its name. */
async function issueIdByName(seed: ParitySeedFacts, projectId: string, cookie: string, name: string): Promise<string> {
  const rows = await serverIssues(seed.workspaceSlug, projectId, cookie);
  const found = rows.find((row) => row.name === name);
  if (!found) throw new Error(`[parity] no server issue named ${JSON.stringify(name)}.`);
  return found.id;
}

/** ISO day (YYYY-MM-DD) `offset` days from today. */
function isoDay(offset: number): string {
  const at = new Date();
  at.setUTCDate(at.getUTCDate() + offset);
  return at.toISOString().slice(0, 10);
}

/**
 * ISO day (YYYY-MM-DD) `offset` days from the local today. The timeline
 * quick-add seeds browser-local dates (the app's own `new Date()`), so
 * expectations about seeded values must use the local clock — the
 * UTC-based isoDay disagrees for part of every day. The spec and the
 * browser share the host clock (no timezone is configured anywhere in
 * the parity harness), so the local days agree.
 */
function localIsoDay(offset: number): string {
  const at = new Date();
  at.setDate(at.getDate() + offset);
  const pad = (part: number): string => String(part).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
}

/**
 * Whether today is the start-of-week-relative last weekday, on which the
 * old app drops the Month today-week highlight (NEWFRONT-163). The spec
 * and the browser share the host clock, so the local weekday agrees with
 * the app's own `new Date()`.
 */
function monthHighlightDeadDay(startOfWeek: number): boolean {
  return (new Date().getDay() + 7 - startOfWeek) % 7 === 6;
}

test(
  specTitle(["ISS-044"], "gantt timeline layout with sidebar and bars"),
  { tag: specTags(["ISS-044"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[0] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: isoDay(-2), target_date: isoDay(5) },
      owner.cookie
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("header shows the count, switcher, today and fullscreen", async () => {
      const header = await driver.ganttHeader();
      expect(header.count).toBe(3);
      expect([...header.views].sort()).toEqual(["Month", "Quarter", "Week"]);
      expect(header.hasToday).toBe(true);
      expect(header.hasFullscreen).toBe(true);
      expect(await driver.boardActiveLayout()).toBe("gantt");
    });

    await test.step("dated issues render bars, undated rows offer add-block", async () => {
      const rows = await driver.ganttSidebarRows();
      for (const row of seed.issueNames) {
        expect(rows.map((entry) => entry.name)).toContain(row);
      }
      expect(rows.find((entry) => entry.name === name)?.identifier).toMatch(/^[A-Z]+-\d+$/);
      expect(rows.find((entry) => entry.name === name)?.duration).toMatch(/\d/);
      expect(await driver.ganttBarExists(name)).toBe(true);
      const plain = seed.issueNames[1] ?? "";
      expect(await driver.ganttBarExists(plain)).toBe(false);
      expect(await driver.ganttRowAddVisible(plain)).toBe(true);
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-045"], "gantt zoom across week, month and quarter"),
  { tag: specTags(["ISS-045"]) },
  async ({ driver, seed }) => {
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("each zoom renders its own day width", async () => {
      expect(await driver.ganttActiveZoom()).toBe("Week");
      const week = await driver.ganttDayWidth();
      await driver.ganttSetZoom("Month");
      expect(await driver.ganttActiveZoom()).toBe("Month");
      // On the week's last day the Month week marker is missing
      // (NEWFRONT-163); the reader falls back to the Current-month pill,
      // which the re-center keeps in view every day.
      expect(await driver.ganttTodayVisible()).toBe(true);
      const month = await driver.ganttDayWidth();
      await driver.ganttSetZoom("Quarter");
      expect(await driver.ganttActiveZoom()).toBe("Quarter");
      expect(await driver.ganttTodayVisible()).toBe(true);
      const quarter = await driver.ganttDayWidth();
      expect(week).toBeGreaterThan(month);
      expect(month).toBeGreaterThan(quarter);
      await driver.ganttSetZoom("Week");
      expect(await driver.ganttTodayVisible()).toBe(true);
    });

    await test.step("weekends tint and weeks start per the user profile", async () => {
      expect(await driver.ganttWeekendTinted()).toBe(true);
      const starts = (await driver.ganttWeekRowStarts()).filter((entry) => entry.length > 0);
      expect(starts.length).toBeGreaterThan(2);
      // The window's edge weeks are cut mid-week; the interior rows all
      // start on the profile's start-of-week day.
      const interior = starts.slice(1, -1);
      expect(new Set(interior).size).toBe(1);
      const weekStart = await serverProfileStartOfWeek(ctx.user.cookie);
      const expected = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][weekStart] ?? "";
      expect(interior[0]?.slice(0, 2)).toBe(expected);
    });

    await test.step("zoom stays session-local across reloads", async () => {
      await driver.ganttSetZoom("Quarter");
      await ganttReloadWithRetry(driver);
      await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
      expect(await driver.ganttActiveZoom()).toBe("Week");
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-046"], "gantt today button and marker"),
  { tag: specTags(["ISS-046"]) },
  async ({ driver, seed }) => {
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("mounting centers today with its marker", async () => {
      expect(await driver.ganttTodayVisible()).toBe(true);
      expect(await driver.ganttTodayHighlighted()).toBe(true);
    });

    await test.step("today re-centers after scrolling away", async () => {
      const width = await driver.ganttTimelineWidth();
      await driver.ganttScrollTo(width);
      await driver.ganttClickToday();
      expect(await driver.ganttTodayVisible()).toBe(true);
    });

    await test.step("every zoom highlights the current column", async () => {
      // NEWFRONT-163: the Month week highlight is missing on the week's
      // last day; Quarter marks the current month every day.
      const deadDay = monthHighlightDeadDay(await serverProfileStartOfWeek(ctx.user.cookie));
      for (const zoom of ["Month", "Quarter"] as const) {
        await driver.ganttSetZoom(zoom);
        expect(await driver.ganttTodayHighlighted()).toBe(zoom === "Month" ? !deadDay : true);
      }
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-046"], "bug: month zoom drops the today highlight on the week's last day (NEWFRONT-163)"),
  { tag: specTags(["ISS-046"]) },
  async ({ driver, seed }) => {
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
    const deadDay = monthHighlightDeadDay(await serverProfileStartOfWeek(ctx.user.cookie));

    await test.step("month marks today except on the dead day", async () => {
      await driver.ganttSetZoom("Month");
      expect(await driver.ganttActiveZoom()).toBe("Month");
      expect(await driver.ganttTodayHighlighted()).toBe(!deadDay);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(specTitle(["ISS-047"], "gantt fullscreen mode"), { tag: specTags(["ISS-047"]) }, async ({ driver, seed }) => {
  const ctx = await openTimeline(driver, seed, seed.projectId);
  await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

  await test.step("the chart toggles between inline and the portal overlay", async () => {
    expect(await driver.ganttFullscreenActive()).toBe(false);
    await driver.ganttToggleFullscreen();
    expect(await driver.ganttFullscreenActive()).toBe(true);
    expect(await driver.ganttTimelineVisible()).toBe(true);
    await driver.ganttToggleFullscreen();
    expect(await driver.ganttFullscreenActive()).toBe(false);
    expect(await driver.ganttTimelineVisible()).toBe(true);
  });

  await test.step("cleanup restores the seed preferences", async () => {
    await restoreTimeline(seed, seed.projectId, ctx);
  });
});

test(
  specTitle(["ISS-048"], "gantt infinite horizontal timeline extension"),
  { tag: specTags(["ISS-048"]) },
  async ({ driver, seed }) => {
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("scrolling past the right edge grows the chart", async () => {
      const before = await driver.ganttTimelineWidth();
      await driver.ganttScrollTo(before);
      await expect.poll(() => driver.ganttTimelineWidth(), { timeout: 60_000 }).toBeGreaterThan(before);
    });

    await test.step("scrolling past the left edge prepends without jumping", async () => {
      const before = await driver.ganttTimelineWidth();
      await driver.ganttScrollTo(0);
      await expect.poll(() => driver.ganttTimelineWidth(), { timeout: 60_000 }).toBeGreaterThan(before);
      // Prepending compensates the offset so the viewport holds still.
      expect(await driver.ganttScrollLeft()).toBeGreaterThan(0);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-049"], "gantt sidebar rows with durations and peek"),
  { tag: specTags(["ISS-049"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[2] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: isoDay(-6), target_date: isoDay(1) },
      owner.cookie
    );
    const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("rows show identifier, name and the dated duration", async () => {
      const rows = await driver.ganttSidebarRows();
      const row = rows.find((entry) => entry.name === name);
      expect(row?.identifier).toBe(`PAR-${details.sequenceId}`);
      expect(row?.duration).toMatch(/\d/);
      const plain = rows.find((entry) => entry.name === (seed.issueNames[0] ?? ""));
      expect(plain?.duration).toBeNull();
    });

    await test.step("clicking a row opens peek", async () => {
      await driver.ganttOpenRowPeek(name);
      expect(await driver.issuePeekVisible()).toBe(true);
      expect(await driver.issuePeekTitle()).toBe(name);
      await driver.issuePeekClose();
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-050"], "reorder issues from the gantt sidebar"),
  { tag: specTags(["ISS-050"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const [first, second, third] = seed.issueNames;
    if (!first || !second || !third) throw new Error("[parity] seed names missing.");
    const ids = await Promise.all(
      [first, second, third].map((name) => issueIdByName(seed, seed.projectId, owner.cookie, name))
    );
    const ranks = await Promise.all(
      ids.map(async (id) => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).sortOrder)
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("manual order persists the new sequence", async () => {
      expect((await driver.ganttSidebarRows()).map((row) => row.name)).toEqual([first, second, third]);
      await driver.ganttDragRowBefore(third, first);
      await expect
        .poll(async () => (await driver.ganttSidebarRows()).map((row) => row.name), { timeout: 60_000 })
        .toEqual([third, first, second]);
      // The sidebar reorder applies optimistically in the UI while the rank
      // PATCH lands asynchronously: a single immediate server read can catch
      // the pre-persist value (the run-10 sweep read 35000 here, and the
      // retry's leaked order proved the PATCH landed moments later). Poll
      // until the new rank persists instead of reading once.
      await expect
        .poll(
          async () =>
            (await serverIssueDetails(seed.workspaceSlug, seed.projectId, ids[2] ?? "", owner.cookie)).sortOrder,
          { timeout: 60_000 }
        )
        .toBeLessThan(ranks[0] ?? 0);
    });

    await test.step("a sorted timeline suppresses the reorder", async () => {
      // Outside manual sort the sidebar DnD instance is disabled: the drop
      // changes neither the row order nor the server ranks, and no toast
      // appears (the row's toast clause was corrected — see its note).
      await setTimelineFilters(driver, seed, seed.projectId, ctx, { order_by: "-created_at" });
      const rows = (await driver.ganttSidebarRows()).map((row) => row.name);
      expect(rows).toHaveLength(3);
      const before = await Promise.all(
        ids.map(
          async (id) => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).sortOrder
        )
      );
      await driver.ganttAttemptRowBefore(rows[2] ?? "", rows[0] ?? "");
      expect((await driver.ganttSidebarRows()).map((row) => row.name)).toEqual(rows);
      const after = await Promise.all(
        ids.map(
          async (id) => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).sortOrder
        )
      );
      expect(after).toEqual(before);
    });

    await test.step("cleanup restores orders and preferences", async () => {
      await Promise.all(
        ids.map((id, index) =>
          serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { sort_order: ranks[index] ?? 0 }, owner.cookie)
        )
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-051"], "move a gantt bar to reschedule"),
  { tag: specTags(["ISS-051"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[0] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    const start = isoDay(-2);
    const target = isoDay(4);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: start, target_date: target },
      owner.cookie
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
    expect(await driver.ganttBarExists(name)).toBe(true);

    await test.step("dragging shifts both dates by the same offset", async () => {
      await driver.ganttDragBar(name, 3);
      // The batch persist lands just after the drop; poll the server truth.
      // (The bar's own post-drop position is racy — NEWFRONT-161 — so the
      // persisted dates, not the pixels, are the assertion.)
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).startDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(1));
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).targetDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(7));
      expect(await driver.ganttBarExists(name)).toBe(true);
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-052"], "resize a gantt bar from the left handle"),
  { tag: specTags(["ISS-052"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[1] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: isoDay(-4), target_date: isoDay(4) },
      owner.cookie
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("hovering previews the handle date", async () => {
      expect(await driver.ganttResizePreview(name, "left")).not.toBeNull();
    });

    await test.step("dragging moves the start and keeps the target", async () => {
      await driver.ganttResizeBar(name, "left", 2);
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).startDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(-2));
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).targetDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(4));
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-053"], "resize a gantt bar from the right handle"),
  { tag: specTags(["ISS-053"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[2] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: isoDay(-4), target_date: isoDay(2) },
      owner.cookie
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("dragging moves the target and keeps the start", async () => {
      await driver.ganttResizeBar(name, "right", 3);
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).startDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(-4));
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).targetDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(5));
    });

    await test.step("a half-dated bar gains its missing date", async () => {
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: null }, owner.cookie);
      await ganttReloadWithRetry(driver);
      await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
      await driver.ganttResizeBar(name, "right", 4);
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).startDate, {
          timeout: 60_000,
        })
        .toBe(isoDay(-4));
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).targetDate, {
          timeout: 60_000,
        })
        .not.toBeNull();
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-054"], "plant a block on an undated timeline row"),
  { tag: specTags(["ISS-054"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const home = await serverDefaultStateId(seed.workspaceSlug, seed.projectId, owner.cookie);
    const weekTitle = `GT plant ${uniqueSuffix().slice(0, 6)}`;
    const weekId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, owner.cookie, weekTitle, home);
    // Both rows are created before opening: the timeline never refetches
    // on its own, so a mid-test create would never render.
    const quarterTitle = `GT quarter ${uniqueSuffix().slice(0, 6)}`;
    const quarterId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, owner.cookie, quarterTitle, home);
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(5);

    await test.step("clicking plants a one-day block in week view", async () => {
      expect(await driver.ganttRowAddVisible(weekTitle)).toBe(true);
      await driver.ganttAddBlock(weekTitle, 2);
      expect(await driver.ganttBarExists(weekTitle)).toBe(true);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, weekId, owner.cookie);
      expect(details.startDate).not.toBeNull();
      expect(details.targetDate).not.toBeNull();
      // The planted block spans start..start+1 (a one-day difference).
      const span = (Date.parse(details.targetDate ?? "") - Date.parse(details.startDate ?? "")) / 86_400_000;
      expect(span).toBe(1);
    });

    await test.step("quarter view plants a week-long block", async () => {
      await driver.ganttSetZoom("Quarter");
      await driver.ganttAddBlock(quarterTitle, 8);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, quarterId, owner.cookie);
      expect(details.startDate).not.toBeNull();
      expect(details.targetDate).not.toBeNull();
      const span = (Date.parse(details.targetDate ?? "") - Date.parse(details.startDate ?? "")) / 86_400_000;
      expect(span).toBe(7);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, quarterId, owner.cookie);
    });

    await test.step("cleanup removes the planted issue and restores preferences", async () => {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, weekId, owner.cookie);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-055"], "quick-add on the gantt timeline"),
  { tag: specTags(["ISS-055"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
    const title = `GT quick ${uniqueSuffix().slice(0, 6)}`;

    await test.step("quick-add seeds today through tomorrow", async () => {
      expect(await driver.ganttHasQuickAdd()).toBe(true);
      await driver.ganttQuickAdd(title);
      expect(await driver.ganttBarExists(title)).toBe(true);
      const id = await issueIdByName(seed, seed.projectId, owner.cookie, title);
      try {
        const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
        expect(details.startDate).toBe(localIsoDay(0));
        expect(details.targetDate).toBe(localIsoDay(1));
      } finally {
        // A failed expectation must not leak the row: every later
        // scenario opens the seed project expecting exactly 3 rows.
        await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      }
    });

    await test.step("cleanup restores the seed preferences", async () => {
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-056"], "gantt bar presentation, preview and open"),
  { tag: specTags(["ISS-056"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const full = seed.issueNames[0] ?? "";
    const fullId = await issueIdByName(seed, seed.projectId, owner.cookie, full);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      fullId,
      { start_date: isoDay(-3), target_date: isoDay(3) },
      owner.cookie
    );
    const half = seed.issueNames[1] ?? "";
    const halfId = await issueIdByName(seed, seed.projectId, owner.cookie, half);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, halfId, { start_date: isoDay(-1) }, owner.cookie);
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("bars tint by state and mask half dates", async () => {
      expect(await driver.ganttBarInfo(full)).toMatchObject({ tinted: true, masked: false, namePinned: true });
      expect(await driver.ganttBarInfo(half)).toMatchObject({ tinted: true, masked: true });
    });

    await test.step("hovering previews and clicking opens peek", async () => {
      await driver.ganttHoverBar(full);
      await expect.poll(() => driver.ganttPreviewVisible(), { timeout: 30_000 }).toBe(true);
      await driver.ganttOpenBarPeek(full);
      expect(await driver.issuePeekVisible()).toBe(true);
      expect(await driver.issuePeekTitle()).toBe(full);
      await driver.issuePeekClose();
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        fullId,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, halfId, { start_date: null }, owner.cookie);
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-057"], "gantt scroll-to-block arrow"),
  { tag: specTags(["ISS-057"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const name = seed.issueNames[2] ?? "";
    const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: isoDay(60), target_date: isoDay(67) },
      owner.cookie
    );
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("off-screen bars offer an arrow that scrolls to them", async () => {
      expect(await driver.ganttBarInView(name)).toBe(false);
      expect(await driver.ganttScrollArrowVisible(name)).toBe(true);
      await driver.ganttClickScrollArrow(name);
      expect(await driver.ganttBarInView(name)).toBe(true);
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-058"], "gantt permission gating for guests"),
  { tag: specTags(["ISS-058"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed carries no guest; re-run the stack seed step.");
    }
    const dated = seed.issueNames[0] ?? "";
    const datedId = await issueIdByName(seed, seed.projectId, owner.cookie, dated);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      datedId,
      { start_date: isoDay(-2), target_date: isoDay(2) },
      owner.cookie
    );
    const plain = seed.issueNames[1] ?? "";
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("members see every affordance", async () => {
      expect(await driver.ganttHandlesVisible(dated)).toBe(true);
      expect(await driver.ganttRowAddVisible(plain)).toBe(true);
      expect(await driver.ganttHasQuickAdd()).toBe(true);
    });

    await test.step("guests see the chart but no editing affordances", async () => {
      // Guests see an empty board until the project lets them use everything;
      // even then the timeline stays view-only with no editing affordances.
      await serverPatchProject(seed.workspaceSlug, seed.projectId, owner.cookie, { guest_view_all_features: true });
      try {
        const guest = await signInFreshUser(seed.guestEmail, seed.guestPassword);
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
          browserCookies(guest)
        );
        await ganttOpenWithRetry(driver);
        await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
        expect(await driver.ganttHandlesVisible(dated)).toBe(false);
        expect(await driver.ganttRowAddVisible(plain)).toBe(false);
        expect(await driver.ganttHasQuickAdd()).toBe(false);
        // Reorder and reschedule attempts change nothing for guests.
        const order = (await driver.ganttSidebarRows()).map((row) => row.name);
        const rankOf = async (name: string): Promise<number> =>
          (
            await serverIssueDetails(
              seed.workspaceSlug,
              seed.projectId,
              await issueIdByName(seed, seed.projectId, owner.cookie, name),
              owner.cookie
            )
          ).sortOrder;
        const ranks = await Promise.all(order.map((name) => rankOf(name)));
        const datesBefore = await serverIssueDetails(seed.workspaceSlug, seed.projectId, datedId, owner.cookie);
        await driver.ganttAttemptRowBefore(order[2] ?? "", order[0] ?? "");
        expect((await driver.ganttSidebarRows()).map((row) => row.name)).toEqual(order);
        await driver.ganttAttemptBarMove(dated, 2);
        const datesAfter = await serverIssueDetails(seed.workspaceSlug, seed.projectId, datedId, owner.cookie);
        expect(datesAfter.startDate).toBe(datesBefore.startDate);
        expect(datesAfter.targetDate).toBe(datesBefore.targetDate);
        await expect
          .poll(async () => Promise.all(order.map((name) => rankOf(name))), { timeout: 30_000 })
          .toEqual(ranks);
      } finally {
        await serverPatchProject(seed.workspaceSlug, seed.projectId, owner.cookie, { guest_view_all_features: false });
      }
    });

    await test.step("cleanup clears the dates and restores preferences", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        datedId,
        { start_date: null, target_date: null },
        owner.cookie
      );
      await restoreTimeline(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-059"], "gantt empty and loading states"),
  { tag: specTags(["ISS-059"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();

    await test.step("an empty project shows the first-run state, not a chart", async () => {
      const emptyId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `GT empty ${suffix}`, `GE${suffix}`);
      // Bypass openTimeline: no chart ever renders to wait for.
      const before = await serverProjectUserProperties(seed.workspaceSlug, emptyId, owner.cookie);
      await serverPatchProjectUserProperties(seed.workspaceSlug, emptyId, owner.cookie, {
        display_filters: {
          ...before.displayFilters,
          layout: "gantt_chart",
          group_by: null,
          order_by: "sort_order",
          sub_group_by: null,
          show_empty_groups: true,
        },
      });
      await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${emptyId}/issues`, browserCookies(owner));
      await expect.poll(() => driver.ganttEmptyVisible(), { timeout: 120_000 }).toBe(true);
      expect(await driver.boardActiveLayout()).toBe("gantt");
      expect(await driver.ganttTimelineVisible()).toBe(false);
      await serverDeleteProject(seed.workspaceSlug, emptyId, owner.cookie);
    });

    await test.step("reloading shows the layout loader before the rows", async () => {
      const ctx = await openTimeline(driver, seed, seed.projectId);
      await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
      // The delayed fetch widens the loading window deterministically; the
      // layout loader is the behavior under test. (The chart's own skeleton
      // rows paint at most one frame behind it, so they stay unasserted.)
      expect(await driver.ganttLoadingObservedOnReload()).toBe(true);
      await restoreTimeline(seed, seed.projectId, ctx);
    });

    await test.step("a 100+ timeline pulses its load-more sentinel", async () => {
      const bigId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `GT bulk ${suffix}`, `GB${suffix}`);
      const home = await serverDefaultStateId(seed.workspaceSlug, bigId, owner.cookie);
      for (let index = 0; index < 105; index += 1) {
        await serverCreateIssue(seed.workspaceSlug, bigId, owner.cookie, `GT bulk ${suffix} ${index}`, home);
      }
      await openTimeline(driver, seed, bigId);
      expect(await driver.ganttLoadMoreObservedOnScroll()).toBe(true);
      await serverDeleteProject(seed.workspaceSlug, bigId, owner.cookie);
    });
  }
);
