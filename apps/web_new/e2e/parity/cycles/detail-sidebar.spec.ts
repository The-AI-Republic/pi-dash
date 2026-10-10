// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-253): cycle detail — switchable work-item
// layouts with per-cycle preferences, the detail header (switcher, count
// badge, analytics shortcut, creation), breadcrumb navigation, the
// collapsible sidebar, inline sidebar date edits, description and lead,
// progress breakdowns with chart and measure, and progress-click
// filtering of the cycle's work items.
//
// Guests are refused the cycle-issues reads server-side (bug
// NEWFRONT-261: the list renders empty with no badge while the sidebar
// counts the items), so the guest scenario pins that refusal; layout
// switching itself stays guest-usable and is pinned alongside.
// Rows: CYC-032–CYC-039.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverAttachCycleIssues,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateEstimate,
  serverCreateIssue,
  serverCreateProjectWithFlags,
  serverCycleAnalytics,
  serverCycleDateCheck,
  serverCycleDetailFull,
  serverCycleIssueIds,
  serverCycleProgress,
  serverCycleUserProperties,
  serverEnsureProjectGuest,
  serverMe,
  serverPatchCycle,
  serverPatchIssue,
  serverProjectCycles,
  serverRequestStatus,
  serverSetProjectEstimate,
  serverStates,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Cycle dates that read CURRENT on the seeded stack (today is 2026-10-10). */
const CURRENT_START = "2026-09-01";
const CURRENT_END = "2026-12-31";
/** Cycle dates that read COMPLETED (finished cycles hide creation). */
const DONE_START = "2026-01-01";
const DONE_END = "2026-02-01";
/** Cycle dates that read UPCOMING (a future start hides the chart). */
const UPCOMING_START = "2026-11-01";
const UPCOMING_END = "2026-11-30";

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Scratch project with the cycles surface enabled; resolves with its id. */
async function createCycleProject(seed: ParitySeedFacts, name: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    name,
    parityProjectIdentifier("N253"),
    { cycleView: true },
    session
  );
}

/** Create one issue in a named state and resolve with its id. */
async function createStateIssue(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  stateName: string,
  session: string
): Promise<string> {
  const states = await serverStates(seed.workspaceSlug, projectId, session);
  const stateId = states.find((row) => row.name.toLowerCase() === stateName)?.id;
  if (stateId === undefined) throw new Error(`[parity] project has no ${stateName} state.`);
  const id = await serverCreateIssue(seed.workspaceSlug, projectId, session, name);
  await serverPatchIssue(seed.workspaceSlug, projectId, id, { state: stateId }, session);
  return id;
}

/**
 * Open a cycle detail page with cookie auth and wait for its sidebar to
 * name the cycle, then for the layout marker to settle (the header
 * switcher renders before the stored selection applies). Cold entry
 * settles: the detail route fetches on mount.
 */
async function openDetail(
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
  await driver.layoutsActiveLayout();
}

/** Most recent toast as one string, polled until non-empty. */
async function toastText(driver: ParityDriver): Promise<string> {
  let text = "";
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        text = toast ? `${toast.title} ${toast.message}` : "";
        return text;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return text;
}

/** Current path without a trailing slash (client-side hops add one). */
async function currentPathClean(driver: ParityDriver): Promise<string> {
  return (await driver.currentPath()).replace(/\/$/, "");
}

/** Stored layout key for one cycle as the server keeps it per user. */
async function cycleLayout(
  seed: ParitySeedFacts,
  projectId: string,
  cycleId: string,
  session: string
): Promise<string | null> {
  return (await serverCycleUserProperties(seed.workspaceSlug, projectId, cycleId, session)).layout;
}

/** Cycle start/end date parts (YYYY-MM-DD) as the server stores them. */
async function cycleDateParts(
  seed: ParitySeedFacts,
  projectId: string,
  cycleId: string,
  session: string
): Promise<{ start: string | null; end: string | null }> {
  const detail = await serverCycleDetailFull(seed.workspaceSlug, projectId, cycleId, session);
  return { start: detail.startDate?.slice(0, 10) ?? null, end: detail.endDate?.slice(0, 10) ?? null };
}

test.describe("cycle detail on desktop", () => {
  test(
    specTitle(["CYC-032"], "detail layouts switch between list, board, calendar, and table"),
    { tag: specTags(["CYC-032"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 switch ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the header offers all five layouts", async () => {
          expect(await driver.layoutsOfferedLayouts()).toEqual([
            "list",
            "kanban",
            "calendar",
            "spreadsheet",
            "gantt_chart",
          ]);
        });

        // Host-level proof only (layout internals belong to the Issues
        // area): the header marker follows each switch and the server
        // stores it per cycle. Container reads stay out because the
        // sidebar's own labels trip the shared board/table heuristics.
        await test.step("board activates and the server stores the switch", async () => {
          await driver.cyclesSwitchLayout("kanban");
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
          await expect.poll(() => cycleLayout(seed, projectId, cycleId, session), { timeout: 30_000 }).toBe("kanban");
        });

        await test.step("calendar activates and renders", async () => {
          await driver.cyclesSwitchLayout("calendar");
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await expect.poll(() => driver.layoutsCalendarVisible(), { timeout: 60_000 }).toBe(true);
        });

        await test.step("table activates", async () => {
          await driver.cyclesSwitchLayout("spreadsheet");
          expect(await driver.layoutsActiveLayout()).toBe("spreadsheet");
        });

        await test.step("list activates again with the issue row", async () => {
          await driver.cyclesSwitchLayout("list");
          expect(await driver.layoutsActiveLayout()).toBe("list");
          // The switch sequence leaves the list grouped by state (the
          // flat "All work items" group is the fresh-list default), so
          // the row is read under its Backlog group once the body
          // catches up with the marker.
          await expect.poll(() => driver.layoutsListGroups(), { timeout: 60_000 }).toContain("Backlog");
          expect(await driver.layoutsListGroupIssueNames("Backlog")).toEqual([`${tag} issue`]);
          await expect.poll(() => cycleLayout(seed, projectId, cycleId, session), { timeout: 30_000 }).toBe("list");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-032"], "The timeline layout renders and survives a reload"),
    { tag: specTags(["CYC-032"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 timeline ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("timeline renders on switch", async () => {
          await driver.cyclesSwitchLayout("gantt_chart");
          expect(await driver.layoutsActiveLayout()).toBe("gantt_chart");
          await expect.poll(() => driver.layoutsGanttVisible(), { timeout: 120_000 }).toBe(true);
        });

        await test.step("a reload keeps the timeline", async () => {
          await driver.layoutsReloadIssues();
          expect(await driver.layoutsActiveLayout()).toBe("gantt_chart");
          await expect.poll(() => driver.layoutsGanttVisible(), { timeout: 120_000 }).toBe(true);
          await expect
            .poll(() => cycleLayout(seed, projectId, cycleId, session), { timeout: 30_000 })
            .toBe("gantt_chart");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-032"], "Each cycle keeps its own layout"),
    { tag: specTags(["CYC-032"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 perprefs ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const nameA = `${tag} cycle A`;
      const nameB = `${tag} cycle B`;
      const idA = await serverCreateCycle(seed.workspaceSlug, projectId, nameA, CURRENT_START, CURRENT_END, session);
      const idB = await serverCreateCycle(seed.workspaceSlug, projectId, nameB, CURRENT_START, CURRENT_END, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, idA, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, idA, nameA, session);

        await test.step("cycle A takes the board", async () => {
          await driver.cyclesSwitchLayout("kanban");
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
          await expect.poll(() => cycleLayout(seed, projectId, idA, session), { timeout: 30_000 }).toBe("kanban");
        });

        await test.step("cycle B takes the calendar without touching A", async () => {
          await openDetail(driver, seed, projectId, idB, nameB, session);
          await driver.cyclesSwitchLayout("calendar");
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await expect.poll(() => cycleLayout(seed, projectId, idB, session), { timeout: 30_000 }).toBe("calendar");
          // A's write settled above; B's write must not touch it.
          expect(await cycleLayout(seed, projectId, idA, session)).toBe("kanban");
        });

        await test.step("returning to A restores the board", async () => {
          await openDetail(driver, seed, projectId, idA, nameA, session);
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-033"], "The switcher jumps between cycles while the badge counts and analytics opens"),
    { tag: specTags(["CYC-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 header ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectName = `${tag} project`;
      const projectId = await createCycleProject(seed, projectName, session);
      const nameA = `${tag} cycle A`;
      const nameB = `${tag} cycle B`;
      const idA = await serverCreateCycle(seed.workspaceSlug, projectId, nameA, CURRENT_START, CURRENT_END, session);
      const idB = await serverCreateCycle(seed.workspaceSlug, projectId, nameB, CURRENT_START, CURRENT_END, session);
      const issueIds: string[] = [];
      for (const name of [`${tag} issue A`, `${tag} issue B`]) {
        issueIds.push(await serverCreateIssue(seed.workspaceSlug, projectId, session, name));
      }
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, idA, issueIds, session);
      try {
        await openDetail(driver, seed, projectId, idA, nameA, session);

        await test.step("the badge counts the cycle's items", async () => {
          await expect.poll(() => driver.cyclesHeaderBadgeText(), { timeout: 60_000 }).toBe("2");
          const progress = await serverCycleProgress(seed.workspaceSlug, projectId, idA, session);
          expect(progress.total).toBe(2);
        });

        await test.step("creation and analytics render for members", async () => {
          expect(await driver.cyclesHeaderAddVisible()).toBe(true);
          expect(await driver.cyclesHeaderAnalyticsVisible()).toBe(true);
        });

        await test.step("the switcher jumps to the other cycle", async () => {
          await driver.cyclesSwitcherOpen(nameA);
          const options = await driver.cyclesSwitcherOptions();
          expect(options).toContain(nameA);
          expect(options).toContain(nameB);
          await driver.cyclesSwitcherPick(nameB);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles/${idB}`);
          expect(await driver.cyclesDetailSidebarName()).toBe(nameB);
          const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
          expect(cycles.map((row) => row.id)).toEqual(expect.arrayContaining([idA, idB]));
        });

        await test.step("an empty cycle shows no badge", async () => {
          expect(await driver.cyclesHeaderBadgeText()).toBe(null);
          const progress = await serverCycleProgress(seed.workspaceSlug, projectId, idB, session);
          expect(progress.total).toBe(0);
        });

        await test.step("analytics opens the analytics view", async () => {
          await driver.cyclesSwitcherOpen(nameB);
          await driver.cyclesSwitcherPick(nameA);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles/${idA}`);
          await driver.cyclesOpenAnalytics();
          expect(await driver.cyclesAnalyticsDialogVisible()).toBe(true);
          expect(await driver.analyticsDialogText()).toContain(`Analytics for ${projectName} in ${nameA}`);
          await driver.cyclesDismissAnalytics();
          expect(await driver.cyclesAnalyticsDialogVisible()).toBe(false);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-033"], "Finished cycles hide creation but keep analytics and the badge"),
    { tag: specTags(["CYC-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 finished ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      // Items attach while the cycle reads current (the server refuses
      // attaches to finished cycles), then the cycle is backdated.
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueIds: string[] = [];
      for (const name of [`${tag} issue A`, `${tag} issue B`]) {
        issueIds.push(await serverCreateIssue(seed.workspaceSlug, projectId, session, name));
      }
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, issueIds, session);
      await serverPatchCycle(
        seed.workspaceSlug,
        projectId,
        cycleId,
        { start_date: DONE_START, end_date: DONE_END },
        session
      );
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("creation is absent", async () => {
          expect(await driver.cyclesHeaderAddVisible()).toBe(false);
        });

        await test.step("analytics and the badge stay", async () => {
          expect(await driver.cyclesHeaderAnalyticsVisible()).toBe(true);
          await expect.poll(() => driver.cyclesHeaderBadgeText(), { timeout: 60_000 }).toBe("2");
          const progress = await serverCycleProgress(seed.workspaceSlug, projectId, cycleId, session);
          expect(progress.total).toBe(2);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(
      ["CYC-032", "CYC-033", "CYC-036"],
      "bug: NEWFRONT-261 guests see an empty list with no badge though items exist"
    ),
    { tag: specTags(["CYC-032", "CYC-033", "CYC-036"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 guest ${Date.now()}`;
      const guest = requireGuest(seed);
      const session = await signInSession(seed.email, seed.password);
      const guestSession = await signInSession(guest.email, guest.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueIds: string[] = [];
      for (const name of [`${tag} issue A`, `${tag} issue B`]) {
        issueIds.push(await serverCreateIssue(seed.workspaceSlug, projectId, session, name));
      }
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, issueIds, session);
      await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, guestSession);

        await test.step("the list stays empty with no badge while items exist", async () => {
          expect(await driver.layoutsListVisible()).toBe(false);
          expect(await driver.layoutsListGroups()).toEqual([]);
          expect(await driver.cyclesHeaderBadgeText()).toBe(null);
          const progress = await serverCycleProgress(seed.workspaceSlug, projectId, cycleId, session);
          expect(progress.total).toBe(2);
          expect(await driver.cyclesProgressWorkItemsText()).toBe("0/2");
        });

        await test.step("the server refuses the guest's item reads", async () => {
          const res = await serverRequestStatus(
            "GET",
            `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`,
            guestSession
          );
          expect(res.status).toBe(403);
        });

        await test.step("creation, analytics, and date edits stay gated (intended)", async () => {
          expect(await driver.cyclesHeaderAddVisible()).toBe(false);
          expect(await driver.cyclesHeaderAnalyticsVisible()).toBe(false);
          expect(await driver.cyclesSidebarDateDisabled()).toBe(true);
        });

        await test.step("layout switching still works and persists per cycle", async () => {
          expect(await driver.layoutsOfferedLayouts()).toEqual([
            "list",
            "kanban",
            "calendar",
            "spreadsheet",
            "gantt_chart",
          ]);
          await driver.cyclesSwitchLayout("kanban");
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
          await driver.layoutsReloadIssues();
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
          await expect
            .poll(() => cycleLayout(seed, projectId, cycleId, guestSession), { timeout: 30_000 })
            .toBe("kanban");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-034"], "Each crumb navigates to its level"),
    { tag: specTags(["CYC-034"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 crumbs ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectName = `${tag} project`;
      const projectId = await createCycleProject(seed, projectName, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the Cycles crumb leads to the cycles list", async () => {
          await driver.cyclesClickCyclesCrumb();
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
          const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
          expect(cycles.map((row) => row.id)).toContain(cycleId);
        });

        await test.step("the project crumb leads to the project's issues", async () => {
          await openDetail(driver, seed, projectId, cycleId, cycleName, session);
          await driver.cyclesClickProjectCrumb(projectName);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/issues`);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-035"], "The sidebar toggle persists across reloads"),
    { tag: specTags(["CYC-035"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 sidebar ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the header toggle hides the sidebar", async () => {
          expect(await driver.cyclesSidebarCollapsedStored()).toBe(false);
          await driver.cyclesToggleSidebarViaHeader();
          await expect.poll(() => driver.cyclesDetailSidebarName(), { timeout: 30_000 }).toBe(null);
          expect(await driver.cyclesSidebarCollapsedStored()).toBe(true);
        });

        await test.step("a reload keeps it hidden", async () => {
          await driver.layoutsReloadIssues();
          await expect.poll(() => driver.cyclesSidebarCollapsedStored(), { timeout: 30_000 }).toBe(true);
          expect(await driver.cyclesDetailSidebarName()).toBe(null);
        });

        await test.step("the header toggle shows it again", async () => {
          await driver.cyclesToggleSidebarViaHeader();
          await expect.poll(() => driver.cyclesDetailSidebarName(), { timeout: 30_000 }).toBe(cycleName);
          expect(await driver.cyclesSidebarCollapsedStored()).toBe(false);
        });

        await test.step("the sidebar's own close control hides it too", async () => {
          await driver.cyclesCloseSidebarViaPanel();
          await expect.poll(() => driver.cyclesDetailSidebarName(), { timeout: 30_000 }).toBe(null);
          expect(await driver.cyclesSidebarCollapsedStored()).toBe(true);
          await driver.cyclesToggleSidebarViaHeader();
          await expect.poll(() => driver.cyclesDetailSidebarName(), { timeout: 30_000 }).toBe(cycleName);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-036"], "Inline date edits save with confirmation"),
    { tag: specTags(["CYC-036"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 dates ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      // Two projects: the overlap guard is project-scoped, so each edit
      // runs without interference from the other's ranges.
      const projectA = await createCycleProject(seed, `${tag} project A`, session);
      const projectB = await createCycleProject(seed, `${tag} project B`, session);
      const nameA = `${tag} cycle A`;
      const nameB = `${tag} cycle B`;
      const idA = await serverCreateCycle(seed.workspaceSlug, projectA, nameA, CURRENT_START, CURRENT_END, session);
      const idB = await serverCreateCycle(seed.workspaceSlug, projectB, nameB, "2026-10-20", "2026-10-30", session);
      try {
        await openDetail(driver, seed, projectA, idA, nameA, session);

        await test.step("the control is editable for members", async () => {
          expect(await driver.cyclesSidebarDateDisabled()).toBe(false);
          expect(await driver.cyclesSidebarDateText()).toContain("Sep 01");
        });

        await test.step("moving the end saves with a success confirmation", async () => {
          // Each calendar click is its own complete range edit; the app
          // fires unordered writes, so the server must reflect one click
          // before the next lands.
          await driver.cyclesSidebarOpenDatePicker();
          await driver.cyclesPickDateDay("October 15th, 2026");
          await expect
            .poll(() => cycleDateParts(seed, projectA, idA, session), { timeout: 30_000 })
            .toEqual({ start: "2026-09-01", end: "2026-10-15" });
          await driver.cyclesPickDateDay("October 18th, 2026");
          await expect
            .poll(() => cycleDateParts(seed, projectA, idA, session), { timeout: 30_000 })
            .toEqual({ start: "2026-09-01", end: "2026-10-18" });
          await driver.cyclesDismissDatePicker();
          expect(await toastText(driver)).toContain("Cycle updated successfully.");
          expect(await driver.cyclesSidebarDateText()).toBe("Sep 01 - Oct 18, 2026");
          expect(
            await serverCycleDateCheck(seed.workspaceSlug, projectA, "2026-09-01", "2026-10-18", idA, session)
          ).toBe(true);
        });

        await test.step("moving the start saves too", async () => {
          await openDetail(driver, seed, projectB, idB, nameB, session);
          await driver.cyclesSidebarOpenDatePicker();
          await driver.cyclesPickDateDay("October 15th, 2026");
          await expect
            .poll(async () => (await cycleDateParts(seed, projectB, idB, session)).start, { timeout: 30_000 })
            .toBe("2026-10-15");
          await driver.cyclesPickDateDay("October 25th, 2026");
          await expect
            .poll(() => cycleDateParts(seed, projectB, idB, session), { timeout: 30_000 })
            .toEqual({ start: "2026-10-15", end: "2026-10-25" });
          await driver.cyclesDismissDatePicker();
          expect(await toastText(driver)).toContain("Cycle updated successfully.");
          expect(await driver.cyclesSidebarDateText()).toBe("Oct 15 - 25, 2026");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectA, session);
        await serverCleanupProject(seed.workspaceSlug, projectB, session);
      }
    }
  );

  test(
    specTitle(["CYC-036"], "Overlapping dates are rejected with guidance"),
    { tag: specTags(["CYC-036"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 overlap ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const nameA = `${tag} cycle A`;
      const idA = await serverCreateCycle(seed.workspaceSlug, projectId, nameA, CURRENT_START, CURRENT_END, session);
      await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} cycle B`, "2026-10-20", "2026-10-30", session);
      try {
        await openDetail(driver, seed, projectId, idA, nameA, session);
        await driver.cyclesSidebarOpenDatePicker();

        await test.step("a clear range saves first", async () => {
          await driver.cyclesPickDateDay("October 15th, 2026");
          await expect
            .poll(() => cycleDateParts(seed, projectId, idA, session), { timeout: 30_000 })
            .toEqual({ start: "2026-09-01", end: "2026-10-15" });
          expect(await toastText(driver)).toContain("Cycle updated successfully.");
        });

        await test.step("extending into the other cycle is rejected", async () => {
          // The rejection toast is lost while the success toast above
          // is still visible (NEWFRONT-264), so the success toast must
          // dismiss before the overlap click lands.
          await expect.poll(() => driver.rulesLastToast(), { timeout: 20_000 }).toBe(null);
          await driver.cyclesPickDateDay("October 25th, 2026");
          let text = "";
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                text = toast ? `${toast.title} ${toast.message}` : "";
                return text;
              },
              { timeout: 30_000 }
            )
            .toMatch(/already have a cycle/);
          expect(text).toContain("Error!");
          await driver.cyclesDismissDatePicker();
          const parts = await cycleDateParts(seed, projectId, idA, session);
          expect(parts.end).toBe("2026-10-15");
          expect(
            await serverCycleDateCheck(seed.workspaceSlug, projectId, "2026-09-01", "2026-10-25", idA, session)
          ).toBe(false);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-037"], "The sidebar shows the description and the lead with an avatar"),
    { tag: specTags(["CYC-037"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 lead ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const me = await serverMe(session);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const description = `${tag} drives the release train`;
      await serverPatchCycle(seed.workspaceSlug, projectId, cycleId, { description }, session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("description text renders", async () => {
          expect(await driver.cyclesSidebarDescription()).toBe(description);
        });

        await test.step("the lead renders with an avatar", async () => {
          const lead = await driver.cyclesSidebarLead();
          expect(lead.name).toBe(me.displayName);
          expect(lead.avatar).toBe(me.displayName.slice(0, 1).toUpperCase());
        });

        await test.step("the server record matches", async () => {
          const detail = await serverCycleDetailFull(seed.workspaceSlug, projectId, cycleId, session);
          expect(detail.description).toBe(description);
          expect(detail.ownedById).toBe(me.id);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-038"], "Counts, chart, and breakdowns render without estimates"),
    { tag: specTags(["CYC-038"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 progress ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const backlog = await createStateIssue(seed, projectId, `${tag} backlog`, "backlog", session);
      const todo = await createStateIssue(seed, projectId, `${tag} todo`, "todo", session);
      const done = await createStateIssue(seed, projectId, `${tag} done`, "done", session);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [backlog, todo, done], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("counts and chart render", async () => {
          expect(await driver.cyclesProgressWorkItemsText()).toBe("1/3");
          expect(await driver.cyclesProgressPointsText()).toBe(null);
          await expect.poll(() => driver.cyclesProgressChartVisible(), { timeout: 60_000 }).toBe(true);
          const analytics = await serverCycleAnalytics(seed.workspaceSlug, projectId, cycleId, "issues", session);
          expect(analytics.completionDays).toBeGreaterThan(0);
        });

        await test.step("no measure dropdown without estimates", async () => {
          expect(await driver.cyclesProgressMeasureValue()).toBe(null);
          expect(await driver.cyclesProgressMeasureOptions()).toEqual([]);
        });

        await test.step("the state breakdown matches the items", async () => {
          await driver.cyclesProgressStatsTab("States");
          let rows: { title: string; percent: number; total: number }[] = [];
          await expect
            .poll(
              async () => {
                rows = await driver.cyclesProgressStatsRows();
                return rows.some((row) => row.title === "Backlog" && row.total === 3);
              },
              { timeout: 60_000 }
            )
            .toBe(true);
          const byTitle = new Map(rows.map((row) => [row.title, row]));
          expect(byTitle.get("Backlog")).toEqual({ title: "Backlog", percent: 33, total: 3 });
          expect(byTitle.get("Unstarted")).toEqual({ title: "Unstarted", percent: 33, total: 3 });
          expect(byTitle.get("Completed")).toEqual({ title: "Completed", percent: 33, total: 3 });
        });

        await test.step("assignee and label breakdowns open", async () => {
          await driver.cyclesProgressStatsTab("Assignees");
          await expect.poll(() => driver.cyclesProgressStatsRows(), { timeout: 60_000 }).not.toEqual([]);
          await driver.cyclesProgressStatsTab("Labels");
          await expect.poll(() => driver.cyclesProgressStatsRows(), { timeout: 60_000 }).not.toEqual([]);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-038"], "The Estimates measure appears where estimates are enabled"),
    { tag: specTags(["CYC-038"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 measure ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const estimate = await serverCreateEstimate(
        seed.workspaceSlug,
        projectId,
        `${tag} est`,
        ["1", "2", "3"],
        session
      );
      await serverSetProjectEstimate(seed.workspaceSlug, projectId, estimate.id, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueId = await createStateIssue(seed, projectId, `${tag} issue`, "backlog", session);
      await serverPatchIssue(
        seed.workspaceSlug,
        projectId,
        issueId,
        { estimate_point: estimate.points[1]?.id },
        session
      );
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the points row and measure dropdown render", async () => {
          expect(await driver.cyclesProgressWorkItemsText()).toBe("0/1");
          await expect.poll(() => driver.cyclesProgressPointsText(), { timeout: 60_000 }).toBe("0/2");
          expect(await driver.cyclesProgressMeasureValue()).toBe("Work items");
          expect(await driver.cyclesProgressMeasureOptions()).toEqual(["Work items", "Estimates"]);
        });

        await test.step("switching measure recounts the breakdown", async () => {
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
          const analytics = await serverCycleAnalytics(seed.workspaceSlug, projectId, cycleId, "points", session);
          expect(analytics.completionDays).toBeGreaterThan(0);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-038"], "Cycles without a valid dated range hide the chart"),
    { tag: specTags(["CYC-038"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 nodates ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const undatedName = `${tag} undated`;
      const undatedId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        undatedName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      await serverPatchCycle(seed.workspaceSlug, projectId, undatedId, { start_date: null, end_date: null }, session);
      const futureName = `${tag} future`;
      const futureId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        futureName,
        UPCOMING_START,
        UPCOMING_END,
        session
      );
      const issueId = await createStateIssue(seed, projectId, `${tag} issue`, "backlog", session);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, futureId, [issueId], session);
      try {
        await test.step("an undated cycle shows the empty notice instead", async () => {
          await openDetail(driver, seed, projectId, undatedId, undatedName, session);
          expect(await driver.cyclesProgressChartVisible()).toBe(false);
          expect(await driver.cyclesProgressEmptyText()).toBe("No Data yet");
          expect(await driver.cyclesProgressMeasureValue()).toBe(null);
          const parts = await cycleDateParts(seed, projectId, undatedId, session);
          expect(parts).toEqual({ start: null, end: null });
        });

        await test.step("a future-start cycle keeps breakdowns but no chart", async () => {
          await openDetail(driver, seed, projectId, futureId, futureName, session);
          expect(await driver.cyclesProgressChartVisible()).toBe(false);
          expect(await driver.cyclesProgressEmptyText()).toBe(null);
          await driver.cyclesProgressStatsTab("States");
          let rows: { title: string; percent: number; total: number }[] = [];
          await expect
            .poll(
              async () => {
                rows = await driver.cyclesProgressStatsRows();
                return rows.some((row) => row.title === "Backlog" && row.total === 1);
              },
              { timeout: 60_000 }
            )
            .toBe(true);
          expect(rows.find((row) => row.title === "Backlog")).toEqual({ title: "Backlog", percent: 100, total: 1 });
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-039"], "Clicking a breakdown entry filters the cycle view"),
    { tag: specTags(["CYC-039"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 clickfilter ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const backlogName = `${tag} backlog`;
      const todoName = `${tag} todo`;
      const doneName = `${tag} done`;
      const backlog = await createStateIssue(seed, projectId, backlogName, "backlog", session);
      const todo = await createStateIssue(seed, projectId, todoName, "todo", session);
      const done = await createStateIssue(seed, projectId, doneName, "done", session);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [backlog, todo, done], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);
        await driver.cyclesSwitchLayout("list");

        await test.step("all three items render unfiltered", async () => {
          await expect
            .poll(() => driver.layoutsListGroupIssueNames("All work items"), { timeout: 60_000 })
            .toEqual(expect.arrayContaining([backlogName, todoName, doneName]));
        });

        await test.step("clicking Backlog narrows the list to it", async () => {
          await driver.cyclesProgressStatsTab("States");
          await driver.cyclesProgressEntryClick("backlog");
          await expect
            .poll(() => driver.layoutsListGroupIssueNames("All work items"), { timeout: 30_000 })
            .toEqual([backlogName]);
          await expect
            .poll(
              async () =>
                JSON.stringify(
                  (await serverCycleUserProperties(seed.workspaceSlug, projectId, cycleId, session)).richFilters
                ),
              { timeout: 30_000 }
            )
            .toContain("backlog");
          expect(await serverCycleIssueIds(seed.workspaceSlug, projectId, cycleId, session)).toEqual(
            expect.arrayContaining([backlog, todo, done])
          );
        });

        await test.step("clicking Completed adds it to the filter", async () => {
          await driver.cyclesProgressEntryClick("completed");
          await expect
            .poll(async () => (await driver.layoutsListGroupIssueNames("All work items")).sort(), { timeout: 30_000 })
            .toEqual([backlogName, doneName].sort());
          await expect
            .poll(
              async () =>
                JSON.stringify(
                  (await serverCycleUserProperties(seed.workspaceSlug, projectId, cycleId, session)).richFilters
                ),
              { timeout: 30_000 }
            )
            .toContain("completed");
        });

        await test.step("clicking Backlog again lifts it", async () => {
          await driver.cyclesProgressEntryClick("backlog");
          await expect
            .poll(() => driver.layoutsListGroupIssueNames("All work items"), { timeout: 30_000 })
            .toEqual([doneName]);
          await expect
            .poll(
              async () =>
                JSON.stringify(
                  (await serverCycleUserProperties(seed.workspaceSlug, projectId, cycleId, session)).richFilters
                ),
              { timeout: 30_000 }
            )
            .toContain("completed");
          await expect
            .poll(
              async () =>
                JSON.stringify(
                  (await serverCycleUserProperties(seed.workspaceSlug, projectId, cycleId, session)).richFilters
                ),
              { timeout: 30_000 }
            )
            .not.toContain("backlog");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );
});

test.describe("cycle detail on small screens", () => {
  test.use({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
  });

  test(
    specTitle(["CYC-032"], "Small screens offer list, board, and calendar layouts"),
    { tag: specTags(["CYC-032"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 mobilelayout ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the layout menu holds three layouts", async () => {
          expect(await driver.cyclesMobileOfferedLayouts()).toEqual(["list", "kanban", "calendar"]);
        });

        await test.step("switching persists across a reload", async () => {
          // Calendar, not the board: board detection reads page text
          // that the sidebar always carries, so only the calendar
          // container read is exact on this page.
          await driver.cyclesMobileSwitchTo("calendar");
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await driver.layoutsReloadIssues();
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await expect.poll(() => cycleLayout(seed, projectId, cycleId, session), { timeout: 30_000 }).toBe("calendar");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-034"], "The small-screen trail goes back"),
    { tag: specTags(["CYC-034"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 mobileback ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createCycleProject(seed, `${tag} project`, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      try {
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${projectId}/cycles`,
          sessionBrowserCookies(session)
        );
        await openDetail(driver, seed, projectId, cycleId, cycleName, session);

        await test.step("the back control returns to the list", async () => {
          expect(await driver.cyclesMobileBackVisible()).toBe(true);
          await driver.cyclesMobileBack();
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["CYC-033"], "Small screens offer analytics to every role"),
    { tag: specTags(["CYC-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF253 mobileguest ${Date.now()}`;
      const guest = requireGuest(seed);
      const session = await signInSession(seed.email, seed.password);
      const guestSession = await signInSession(guest.email, guest.password);
      const projectName = `${tag} project`;
      const projectId = await createCycleProject(seed, projectName, session);
      const cycleName = `${tag} cycle`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        cycleName,
        CURRENT_START,
        CURRENT_END,
        session
      );
      await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
      try {
        await openDetail(driver, seed, projectId, cycleId, cycleName, guestSession);

        await test.step("guests open analytics from the bar", async () => {
          await driver.cyclesMobileOpenAnalytics();
          expect(await driver.cyclesAnalyticsDialogVisible()).toBe(true);
          expect(await driver.analyticsDialogText()).toContain(`Analytics for ${projectName} in ${cycleName}`);
          await driver.cyclesDismissAnalytics();
          expect(await driver.cyclesAnalyticsDialogVisible()).toBe(false);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );
});
