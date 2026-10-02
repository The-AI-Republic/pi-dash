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
  await driver.ganttOpenTimeline();
  return { user, beforeFilters: before.displayFilters, beforeProperties: before.displayProperties };
}

/** Restore the exact preferences an opened timeline started with. */
async function restoreTimeline(seed: ParitySeedFacts, projectId: string, ctx: TimelineContext): Promise<void> {
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
    display_filters: ctx.beforeFilters,
    display_properties: ctx.beforeProperties,
  });
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
      const month = await driver.ganttDayWidth();
      await driver.ganttSetZoom("Quarter");
      expect(await driver.ganttActiveZoom()).toBe("Quarter");
      const quarter = await driver.ganttDayWidth();
      expect(week).toBeGreaterThan(month);
      expect(month).toBeGreaterThan(quarter);
      await driver.ganttSetZoom("Week");
    });

    await test.step("weekends tint and weeks start per the user profile", async () => {
      expect(await driver.ganttWeekendTinted()).toBe(true);
      const starts = await driver.ganttWeekRowStarts();
      expect(starts.length).toBeGreaterThan(0);
      const first = starts[0] ?? "";
      expect(new Set(starts).size).toBe(1);
      const weekStart = await serverProfileStartOfWeek(ctx.user.cookie);
      const expected = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][weekStart] ?? "";
      expect(first.slice(0, 2)).toBe(expected);
    });

    await test.step("zoom stays session-local across reloads", async () => {
      await driver.ganttSetZoom("Quarter");
      await driver.boardReloadIssues();
      await driver.ganttOpenTimeline();
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
      for (const zoom of ["Month", "Quarter"] as const) {
        await driver.ganttSetZoom(zoom);
        expect(await driver.ganttTodayHighlighted()).toBe(true);
      }
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
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      expect(details.startDate).toBe(isoDay(1));
      expect(details.targetDate).toBe(isoDay(7));
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
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      expect(details.startDate).toBe(isoDay(-2));
      expect(details.targetDate).toBe(isoDay(4));
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
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      expect(details.startDate).toBe(isoDay(-4));
      expect(details.targetDate).toBe(isoDay(5));
    });

    await test.step("a half-dated bar gains its missing date", async () => {
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: null }, owner.cookie);
      await driver.boardReloadIssues();
      await driver.ganttOpenTimeline();
      await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
      await driver.ganttResizeBar(name, "right", 4);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      expect(details.startDate).toBe(isoDay(-4));
      expect(details.targetDate).not.toBeNull();
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
    const ctx = await openTimeline(driver, seed, seed.projectId);
    await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(4);

    await test.step("clicking plants a one-day block in week view", async () => {
      expect(await driver.ganttRowAddVisible(weekTitle)).toBe(true);
      await driver.ganttAddBlock(weekTitle, 2);
      expect(await driver.ganttBarExists(weekTitle)).toBe(true);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, weekId, owner.cookie);
      expect(details.startDate).not.toBeNull();
      expect(details.targetDate).toBe(details.startDate);
    });

    await test.step("quarter view plants a week-long block", async () => {
      const quarterTitle = `GT quarter ${uniqueSuffix().slice(0, 6)}`;
      const quarterId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, owner.cookie, quarterTitle, home);
      await driver.ganttSetZoom("Quarter");
      await driver.ganttAddBlock(quarterTitle, 2);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, quarterId, owner.cookie);
      expect(details.startDate).not.toBeNull();
      expect(details.targetDate).not.toBeNull();
      const span = (Date.parse(details.targetDate ?? "") - Date.parse(details.startDate ?? "")) / 86_400_000;
      expect(span).toBe(6);
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
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      expect(details.startDate).toBe(isoDay(0));
      expect(details.targetDate).toBe(isoDay(1));
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie);
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
      const guest = await signInFreshUser(seed.guestEmail, seed.guestPassword);
      await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(guest));
      await driver.ganttOpenTimeline();
      await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
      expect(await driver.ganttHandlesVisible(dated)).toBe(false);
      expect(await driver.ganttRowAddVisible(plain)).toBe(false);
      expect(await driver.ganttHasQuickAdd()).toBe(false);
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
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `GT empty ${suffix}`, `GE${suffix}`);
    await openTimeline(driver, seed, projectId);
    await expect.poll(() => driver.ganttTimelineVisible(), { timeout: 120_000 }).toBe(true);

    await test.step("an empty project shows no rows and a zero count", async () => {
      expect(await driver.ganttSidebarRows()).toHaveLength(0);
      expect((await driver.ganttHeader()).count).toBe(0);
    });

    await test.step("reloading shows sidebar skeletons before the rows", async () => {
      // The delayed fetch widens the loading window deterministically; the
      // skeleton itself is the behavior under test.
      expect(await driver.ganttLoadingObservedOnReload()).toBe(true);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);
