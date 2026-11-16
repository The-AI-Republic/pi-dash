// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-325): estimates consumers — the work-item
// picker following the active system, project-wide estimate gating, cycle
// progress grouping by estimate totals, the analytics estimate axis, palette
// assignment, read-only rendering, and estimate history entries.
// Rows: EST-017–EST-023.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverArchiveIssue,
  serverAttachCycleIssues,
  serverCleanupProject,
  serverClearProjectEstimate,
  serverCreateCycle,
  serverCreateEstimate,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCycleAnalytics,
  serverHistory,
  serverIssue,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectEstimate,
  serverProjectStates,
  serverSetProjectEstimate,
  serverUnarchiveIssue,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Scratch project; resolves with its id. */
async function createEstimateProject(
  seed: ParitySeedFacts,
  name: string,
  session: string,
  flags: { cycleView?: boolean } = {}
): Promise<string> {
  return serverCreateProjectWithFlags(seed.workspaceSlug, name, parityProjectIdentifier("N325"), flags, session);
}

/** Estimate history entries for one issue (assign/replace/clear shapes). */
async function estimateHistoryEntries(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  session: string
): Promise<{ field: unknown; verb: unknown; oldValue: unknown; newValue: unknown }[]> {
  const probe = await serverHistory(workspaceSlug, projectId, issueId, session, "?activity_type=issue-property");
  return probe.entries
    .filter((entry) => typeof entry.field === "string" && entry.field.startsWith("estimate"))
    .map((entry) => ({ field: entry.field, verb: entry.verb, oldValue: entry.old_value, newValue: entry.new_value }));
}

/** Open a cycle detail page and settle on its sidebar (cycles-spec shape). */
async function openCycleDetail(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  cycleId: string,
  cycleName: string,
  session: string
): Promise<void> {
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}`,
    sessionBrowserCookies(session)
  );
  await expect.poll(() => driver.cyclesDetailSidebarName(), { timeout: 120_000 }).toBe(cycleName);
  // Settle on the layout switcher where it exists: the oracle's cycle
  // page always carries one, while web_new's cycle page may not (cycle
  // layouts are proven by the Cycles/Issues suites, not EST-019). The
  // sidebar-name poll above is this helper's real settle.
  if (driver.target === "web") {
    await driver.layoutsActiveLayout();
  }
}

/** Open a DIFFERENT issue than the one on screen (openIssueDetail reloads when already on a browse URL, so step off the current detail first or the new issue never loads). */
async function openOtherIssueDetail(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  issueId: string
): Promise<void> {
  await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
  await driver.openIssueDetail(seed.workspaceSlug, projectId, issueId);
}

/** Create one issue in a named state group and resolve with its id. */
async function createGroupedIssue(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  group: string,
  session: string
): Promise<string> {
  const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
  const state = states.find((row) => row.group === group);
  if (state === undefined) throw new Error(`[parity] project has no ${group} state.`);
  const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, name, session);
  await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: state.id }, session);
  return issue.id;
}

test(
  specTitle(
    ["EST-017"],
    "estimate picker follows the active system; assigning persists with history, clearing persists"
  ),
  { tag: specTags(["EST-017"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 picker ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const points = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2", "5"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, points.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // A second scratch project carrying a categories-family system. One
    // system per project is how real projects hold them: the client
    // resolves the active system per project, so a second project is the
    // faithful way to show the offered values follow the active system.
    const sizesProjectId = await createEstimateProject(seed, `${tag} sizes project`, session);
    const sizes = await serverCreateEstimate(
      seed.workspaceSlug,
      sizesProjectId,
      "Sizes",
      ["Big", "Small"],
      session,
      "categories"
    );
    await serverSetProjectEstimate(seed.workspaceSlug, sizesProjectId, sizes.id, session);
    const sizesIssue = await serverCreateIssueFull(seed.workspaceSlug, sizesProjectId, `${tag} sizes issue`, session);
    expect(sizes.points.map((row) => row.value)).toEqual(expect.arrayContaining(["Big", "Small"]));
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the picker lists the active points values", async () => {
        await driver.propertyOpenPicker("Estimate");
        expect(await driver.pickerHasSearch()).toBe(true);
        // The options load after the picker opens; poll for the
        // settled set instead of reading mid-load (EST-017).
        await expect
          .poll(() => driver.pickerOptionTexts(), { timeout: 30_000 })
          .toEqual(["No estimate", "1", "2", "5"]);
        await driver.pickerClickOutside();
      });

      await test.step("a categories-family project offers its own values", async () => {
        await openOtherIssueDetail(driver, seed, sizesProjectId, sizesIssue.id);
        await driver.propertyOpenPicker("Estimate");
        // Poll for the settled set: see the step above (EST-017).
        await expect
          .poll(() => driver.pickerOptionTexts(), { timeout: 30_000 })
          .toEqual(["No estimate", "Big", "Small"]);
        await driver.pickerClickOutside();
      });

      await test.step("assigning persists and records a history entry", async () => {
        // "Small" is unambiguous for the picker's substring match ("M"
        // alone would hit "No estimate" first).
        const wanted = "Small";
        await driver.openIssueDetail(seed.workspaceSlug, sizesProjectId, sizesIssue.id);
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick(wanted);
        await expect
          .poll(
            async () => (await serverIssue(seed.workspaceSlug, sizesProjectId, sizesIssue.id, session)).estimate_point,
            {
              timeout: 30_000,
            }
          )
          .not.toBe(null);
        await expect
          .poll(() => estimateHistoryEntries(seed.workspaceSlug, sizesProjectId, sizesIssue.id, session), {
            timeout: 90_000,
          })
          .toEqual(expect.arrayContaining([expect.objectContaining({ verb: "updated", newValue: wanted })]));
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 30_000 }).toContain(wanted);
        // The feed renders entries on mount (no live push), so re-open
        // before reading it.
        await driver.openIssueDetail(seed.workspaceSlug, sizesProjectId, sizesIssue.id);
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 90_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(wanted)]));
      });

      await test.step("clearing persists", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("No estimate");
        await expect
          .poll(
            async () => (await serverIssue(seed.workspaceSlug, sizesProjectId, sizesIssue.id, session)).estimate_point,
            {
              timeout: 30_000,
            }
          )
          .toBe(null);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 30_000 }).toContain("None");
        // Clearing records no history entry (bug NEWFRONT-345); the
        // companion bug: scenario pins that absence for EST-017/EST-023.
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, sizesProjectId, session);
    }
  }
);

test(
  specTitle(["EST-017", "EST-023"], "bug: clearing an estimate records no history entry (NEWFRONT-345)"),
  { tag: specTags(["EST-017", "EST-023"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 clearbug ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("assigning records the baseline entry", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("3");
        const wanted = system.points.find((row) => row.value === "3");
        expect(wanted).toBeDefined();
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBe(wanted?.id);
        await expect
          .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), { timeout: 120_000 })
          .toEqual(expect.arrayContaining([expect.objectContaining({ verb: "updated", newValue: "3" })]));
      });
      const baseline = await estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session);

      await test.step("clearing adds no entry within a bounded window", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("No estimate");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBe(null);
        // The worker crashes on the empty replacement (NEWFRONT-345),
        // so the clear adds no entry. Pin that over a bounded window:
        // the assign path records its entry well inside it. If the
        // worker is fixed this fails — unmark the scenario and assert
        // the removed entry instead.
        const deadline = Date.now() + 45_000;
        let entries = baseline;
        while (Date.now() < deadline) {
          entries = await estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session);
          if (entries.length > baseline.length) break;
          await new Promise((resolve) => setTimeout(resolve, 5000));
        }
        expect(entries).toEqual(baseline);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        const texts = await driver.activityEntryTexts();
        expect(texts.some((text) => /removed the estimate/i.test(text))).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-018"], "estimate controls follow the project flag on the sidebar and the spreadsheet"),
  { tag: specTags(["EST-018"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 gating ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // The spreadsheet layout is set once up front; every later flip
    // touches only the project estimate flag.
    await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
      display_filters: { layout: "spreadsheet", group_by: null, order_by: "sort_order" },
    });
    const sheetHeaders = async (): Promise<string[]> => {
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${projectId}/issues`,
        sessionBrowserCookies(session)
      );
      await expect.poll(() => driver.layoutsSpreadsheetVisible(), { timeout: 120_000 }).toBe(true);
      return driver.layoutsSheetHeaders();
    };
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("with estimates on, the sidebar row and sheet column show", async () => {
        expect(await serverProjectEstimate(seed.workspaceSlug, projectId, session)).toBe(system.id);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        // The sidebar's rows render after the panel hydrates; poll for
        // the settled row instead of reading mid-load (EST-018).
        await expect.poll(() => driver.propertyRowPresent("Estimate"), { timeout: 30_000 }).toBe(true);
        expect(await sheetHeaders()).toContain("Estimate");
      });

      await test.step("with estimates off, neither surface offers a control", async () => {
        await serverClearProjectEstimate(seed.workspaceSlug, projectId, session);
        expect(await serverProjectEstimate(seed.workspaceSlug, projectId, session)).toBe(null);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        expect(await driver.propertyRowPresent("Estimate")).toBe(false);
        expect(await sheetHeaders()).not.toContain("Estimate");
      });

      await test.step("switching on brings both back with no other configuration", async () => {
        await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
        expect(await serverProjectEstimate(seed.workspaceSlug, projectId, session)).toBe(system.id);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        // Poll for the settled row: see the first step (EST-018).
        await expect.poll(() => driver.propertyRowPresent("Estimate"), { timeout: 30_000 }).toBe(true);
        expect(await sheetHeaders()).toContain("Estimate");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-019"], "cycle progress offers estimate totals on numeric-family projects"),
  { tag: specTags(["EST-019"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 cycnum ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session, { cycleView: true });
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const cycleName = `${tag} cycle`;
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      cycleName,
      "2026-09-01",
      "2026-12-31",
      session
    );
    const issueId = await createGroupedIssue(seed, projectId, `${tag} issue`, "backlog", session);
    const point = system.points.find((row) => row.value === "2");
    expect(point).toBeDefined();
    await serverPatchIssue(seed.workspaceSlug, projectId, issueId, { estimate_point: point?.id }, session);
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openCycleDetail(driver, seed, projectId, cycleId, cycleName, session);

      await test.step("both groupings are offered with counts by default", async () => {
        expect(await driver.cyclesProgressWorkItemsText()).toBe("0/1");
        await expect.poll(() => driver.cyclesProgressPointsText(), { timeout: 60_000 }).toBe("0/2");
        expect(await driver.cyclesProgressMeasureValue()).toBe("Work items");
        expect(await driver.cyclesProgressMeasureOptions()).toEqual(["Work items", "Estimates"]);
      });

      await test.step("switching to estimates recounts the breakdown", async () => {
        await driver.cyclesProgressStatsTab("States");
        await driver.cyclesProgressPickMeasure("Estimates");
        let rows: { title: string; percent: number; total: number }[] = [];
        await expect
          .poll(
            async () => {
              rows = await driver.cyclesProgressStatsRows();
              return rows.some((row) => row.title === "Backlog" && row.total === 2);
            },
            { timeout: 60_000 }
          )
          .toBe(true);
        expect(rows.find((row) => row.title === "Backlog")).toEqual({ title: "Backlog", percent: 100, total: 2 });
        expect(await driver.cyclesProgressChartVisible()).toBe(true);
        const byCount = await serverCycleAnalytics(seed.workspaceSlug, projectId, cycleId, "issues", session);
        const byPoints = await serverCycleAnalytics(seed.workspaceSlug, projectId, cycleId, "points", session);
        expect(byCount.completionDays).toBeGreaterThan(0);
        expect(byPoints.completionDays).toBeGreaterThan(0);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-019"], "cycle progress hides the grouping on text-family projects"),
  { tag: specTags(["EST-019"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 cyccat ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session, { cycleView: true });
    const system = await serverCreateEstimate(
      seed.workspaceSlug,
      projectId,
      "Sizes",
      ["S", "M"],
      session,
      "categories"
    );
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const cycleName = `${tag} cycle`;
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      cycleName,
      "2026-09-01",
      "2026-12-31",
      session
    );
    const issueId = await createGroupedIssue(seed, projectId, `${tag} issue`, "backlog", session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issueId, { estimate_point: system.points[0]?.id }, session);
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openCycleDetail(driver, seed, projectId, cycleId, cycleName, session);

      await test.step("counts render with no grouping control", async () => {
        expect(await driver.cyclesProgressWorkItemsText()).toBe("0/1");
        expect(await driver.cyclesProgressPointsText()).toBe(null);
        expect(await driver.cyclesProgressMeasureValue()).toBe(null);
        expect(await driver.cyclesProgressMeasureOptions()).toEqual([]);
        const analytics = await serverCycleAnalytics(seed.workspaceSlug, projectId, cycleId, "issues", session);
        expect(analytics.completionDays).toBeGreaterThan(0);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-020"], "analytics offers the estimate point axis and aggregates charts by estimate value"),
  { tag: specTags(["EST-020"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 ax ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issueA = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue A`, session);
    const issueB = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue B`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issueA.id, { estimate_point: system.points[0]?.id }, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issueB.id, { estimate_point: system.points[1]?.id }, session);
    const offProjectId = await createEstimateProject(seed, `${tag} off project`, session);
    await serverCreateIssueFull(seed.workspaceSlug, offProjectId, `${tag} off issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the y-axis offers no estimate metric; the x-axis offers the estimate point", async () => {
        await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
        await driver.estimatesOpenAnalytics();
        expect(await driver.estimatesAnalyticsYAxisValue()).toBe("Work item");
        expect(await driver.estimatesAnalyticsYAxisOptions()).toEqual(["Work item"]);
        expect(await driver.estimatesAnalyticsXAxisValue()).toBe("Priority");
        expect(await driver.estimatesAnalyticsXAxisOptions()).toContain("Estimate point");
      });

      await test.step("grouping by estimate point aggregates one row per value", async () => {
        await driver.estimatesAnalyticsPickXAxis("Estimate point");
        let rows: string[][] = [];
        await expect
          .poll(
            async () => {
              rows = await driver.estimatesAnalyticsTableRows();
              return (
                rows.some((row) => row[0] === "1" && row[1] === "1") &&
                rows.some((row) => row[0] === "3" && row[1] === "1")
              );
            },
            { timeout: 90_000 }
          )
          .toBe(true);
        expect(rows[0]).toEqual(["Estimate point", "Count"]);
        const stored = await Promise.all(
          [issueA.id, issueB.id].map(
            async (issueId) => (await serverIssue(seed.workspaceSlug, projectId, issueId, session)).estimate_point
          )
        );
        expect(stored).toEqual([system.points[0]?.id, system.points[1]?.id]);
        await driver.closeAnalytics();
      });

      await test.step("without estimates the y-axis still offers no estimate metric", async () => {
        await driver.openProjectIssuesSettled(seed.workspaceSlug, offProjectId);
        await driver.estimatesOpenAnalytics();
        expect(await driver.estimatesAnalyticsYAxisOptions()).toEqual(["Work item"]);
        await driver.closeAnalytics();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, offProjectId, session);
    }
  }
);

test(
  specTitle(["EST-021"], "palette assigns the estimate with picker parity"),
  { tag: specTags(["EST-021"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 palette ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const offProjectId = await createEstimateProject(seed, `${tag} off project`, session);
    const offIssue = await serverCreateIssueFull(seed.workspaceSlug, offProjectId, `${tag} off issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the estimate menu lists the active values", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.paletteHasCommand("Change estimate"), { timeout: 30_000 }).toBe(true);
        await driver.activatePaletteCommand("Change estimate");
        // web_new marks the current value with a trailing selection
        // check; normalize it on both legs (the oracle titles carry
        // none) and assert the offered set exactly.
        const titles = (await driver.paletteCommandTitles()).map((title) => title.replace(/✓$/, ""));
        expect(titles).toEqual(["No estimate", "1", "2", "3"]);
      });

      await test.step("choosing a value persists it with picker parity", async () => {
        const wanted = system.points.find((row) => row.value === "2");
        expect(wanted).toBeDefined();
        await driver.activatePaletteCommand("2");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBe(wanted?.id);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 30_000 }).toContain("2");
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 90_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining("2")]));
      });

      await test.step("without estimates the palette offers no estimate command", async () => {
        await openOtherIssueDetail(driver, seed, offProjectId, offIssue.id);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
        expect(await driver.paletteHasCommand("Change estimate")).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, offProjectId, session);
    }
  }
);

test(
  specTitle(
    ["EST-022"],
    "bug: archived peek disables the estimate row but shows None though a value is set (NEWFRONT-349)"
  ),
  { tag: specTags(["EST-022"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 readonly ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const point = system.points.find((row) => row.value === "2");
    expect(point).toBeDefined();
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: point?.id }, session);
    const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
    const doneState = states.find((row) => row.group === "completed" || row.group === "cancelled");
    if (doneState === undefined) throw new Error("[parity] project has no completed state.");
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: doneState.id }, session);
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the archived peek offers no estimate edit affordance", async () => {
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${projectId}/archives/issues?peekIssueId=${issue.id}`,
          sessionBrowserCookies(session)
        );
        await expect.poll(() => driver.peekOpen(), { timeout: 120_000 }).toBe(true);
        // The peek's property rows render after the panel opens; poll
        // for the settled row instead of reading mid-load (EST-022).
        await expect.poll(() => driver.propertyRowPresent("Estimate"), { timeout: 30_000 }).toBe(true);
        expect(await driver.propertyTriggerPresent("Estimate")).toBe(true);
        expect(await driver.propertyPickerDisabled("Estimate")).toBe(true);
        expect(await driver.pickerOpen()).toBe(false);
      });

      await test.step("visiting changes nothing server-side", async () => {
        // The live issue read excludes archived rows, so restore first
        // and then confirm the point survived the visit untouched.
        await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point).toBe(point?.id);
      });

      await test.step("the set value reads as the empty placeholder (NEWFRONT-349)", async () => {
        // Intended: the row shows "2" read-only. The archived peek
        // context never loads the estimates store, so the label falls
        // back to the placeholder. If fixed this fails — drop the bug:
        // marker and assert the value instead. web_new loads the store
        // in the archived peek, so the bug is fixed there while the
        // oracle still pins it: each leg asserts its own behavior.
        if (driver.target === "web_new") {
          expect(await driver.propertyValueText("Estimate")).toContain("2");
        } else {
          expect(await driver.propertyValueText("Estimate")).toContain("None");
        }
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-023"], "estimate changes render history entries naming the values with working entry links"),
  { tag: specTags(["EST-023"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF325 history ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await createEstimateProject(seed, `${tag} project`, session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("assigning renders a set entry naming the value", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("1");
        // The history write lands asynchronously while the feed renders
        // once on mount: wait for the server-side entry before remounting
        // the feed, or the fresh mount can miss it (EST-023).
        await expect
          .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), { timeout: 90_000 })
          .toEqual(
            expect.arrayContaining([
              expect.objectContaining({ field: "estimate_points", verb: "updated", oldValue: null, newValue: "1" }),
            ])
          );
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 90_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining("set the estimate point to 1")]));
      });

      await test.step("replacing records old and new values", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("3");
        // Server-side first: see the step above (EST-023).
        await expect
          .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), { timeout: 90_000 })
          .toEqual(
            expect.arrayContaining([
              expect.objectContaining({ field: "estimate_points", verb: "updated", oldValue: "1", newValue: "3" }),
            ])
          );
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 90_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining("set the estimate point to 3")]));
      });

      await test.step("entry links navigate", async () => {
        const url = await driver.activityOpenFirstEntryLink();
        expect(url).toMatch(/profile/);
        // Clearing records no removed entry (bug NEWFRONT-345); the
        // companion bug: scenario pins that absence for EST-017/EST-023.
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
