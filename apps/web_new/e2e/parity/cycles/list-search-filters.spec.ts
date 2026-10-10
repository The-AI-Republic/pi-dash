// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-249): live cycles list — hero plus grouped
// rows with counts and collapsing, per-row markers, detail addresses,
// the peek query key, expanding search, lifecycle-state and date-window
// filters, and the applied-chip row with clear-all.
// Rows: CYC-001–CYC-008.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddCycleFavorite,
  serverArchiveCycle,
  serverArchivedCycles,
  serverAttachCycleIssues,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCycleDetail,
  serverCycleIsFavorite,
  serverCycleIssueIds,
  serverPatchCycle,
  serverProjectCycles,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Cycle dates that read CURRENT on the seeded stack (today is 2026-10-10). */
const CURRENT_START = "2026-09-01";
const CURRENT_END = "2026-12-31";
/** Short current end so end-date windows can tell current from upcoming. */
const CURRENT_END_SHORT = "2026-10-20";
/** Cycle dates that read UPCOMING. */
const UPCOMING_START = "2026-11-01";
const UPCOMING_END = "2026-12-31";
/** Cycle dates that read COMPLETED. */
const DONE_START = "2026-01-01";
const DONE_END = "2026-02-01";

/**
 * Create a cycle reading DRAFT: the create call takes dated input, then a
 * patch clears both dates. Resolves with the cycle id.
 */
async function createDraftCycle(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  session: string
): Promise<string> {
  const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
  await serverPatchCycle(seed.workspaceSlug, projectId, id, { start_date: null, end_date: null }, session);
  return id;
}

/**
 * Open the live list until `wanted` names render, retrying the whole
 * navigation once: the oracle dev server stalls whole renders under
 * shared-stack load, and a stalled first pass must not fail an assertion.
 * Genuinely absent rows still fail.
 */
async function openListSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  wanted: string[]
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    try {
      await driver.cyclesOpenList(workspaceSlug, projectId);
      await expect
        .poll(
          async () => {
            const names = await driver.cyclesVisibleNames();
            return wanted.every((name) => names.includes(name));
          },
          { timeout: 30_000 }
        )
        .toBe(true);
      return;
    } catch (error) {
      if (round === 2) throw error;
    }
  }
}

/**
 * Expand the completed group (it starts collapsed) until `name` renders.
 * Tests that read finished rows call this after opening the list.
 */
async function ensureCompletedExpanded(driver: ParityDriver, name: string): Promise<void> {
  if (!(await driver.cyclesGroupExpanded("Completed"))) {
    await driver.cyclesGroupToggle("Completed");
  }
  await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(name);
}

test(
  specTitle(["CYC-001"], "cycles list leads with the hero, then counted collapsible groups"),
  { tag: specTags(["CYC-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 hero ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const current = `${tag} current`;
    const upcoming = `${tag} upcoming`;
    const draft = `${tag} draft`;
    const done = `${tag} done`;
    await serverCreateCycle(seed.workspaceSlug, projectId, current, CURRENT_START, CURRENT_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, upcoming, UPCOMING_START, UPCOMING_END, session);
    await createDraftCycle(seed, projectId, draft, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, done, DONE_START, DONE_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [current, upcoming, draft]);

      await test.step("the hero names the current cycle and renders first", async () => {
        expect(await driver.cyclesHeroName()).toBe(current);
        const names = await driver.cyclesVisibleNames();
        expect(names[0]).toBe(current);
      });

      await test.step("both groups render with correct counts", async () => {
        const headings = await driver.cyclesGroupHeadings();
        expect(headings.length).toBe(3);
        expect(headings[0]).toMatch(/^Active cycle/);
        expect(headings[1]).toMatch(/^Upcoming cycle/);
        expect(headings[2]).toMatch(/^Completed cycle/);
        // Upcoming holds upcoming plus draft; completed holds finished only.
        expect(await driver.cyclesGroupCounts()).toEqual({ upcoming: 2, completed: 1 });
        // Active and upcoming start expanded; completed starts collapsed.
        expect(await driver.cyclesGroupExpanded("Active")).toBe(true);
        expect(await driver.cyclesGroupExpanded("Upcoming")).toBe(true);
        expect(await driver.cyclesGroupExpanded("Completed")).toBe(false);
        await ensureCompletedExpanded(driver, done);
      });

      await test.step("collapsing the upcoming group hides its rows only", async () => {
        expect(await driver.cyclesGroupExpanded("Upcoming")).toBe(true);
        await driver.cyclesGroupToggle("Upcoming");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).not.toContain(upcoming);
        const names = await driver.cyclesVisibleNames();
        expect(names).not.toContain(draft);
        expect(names).toContain(current);
        expect(names).toContain(done);
        await driver.cyclesGroupToggle("Upcoming");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(upcoming);
      });

      await test.step("collapsing the completed group hides its rows only", async () => {
        await driver.cyclesGroupToggle("Completed");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).not.toContain(done);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(current);
        expect(names).toContain(upcoming);
        await driver.cyclesGroupToggle("Completed");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(done);
      });

      await test.step("the server agrees on lifecycle membership", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        const statusOf = (name: string): string => cycles.find((row) => row.name === name)?.status.toLowerCase() ?? "";
        expect(statusOf(current)).toBe("current");
        expect(statusOf(upcoming)).toBe("upcoming");
        expect(statusOf(draft)).toBe("draft");
        expect(statusOf(done)).toBe("completed");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-002"], "cycle rows carry the ring, dates, extras, and favorite marker"),
  { tag: specTags(["CYC-002"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 markers ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const current = `${tag} current`;
    const upcoming = `${tag} upcoming`;
    const draft = `${tag} draft`;
    const done = `${tag} done`;
    const currentId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      current,
      CURRENT_START,
      CURRENT_END,
      session
    );
    const upcomingId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      upcoming,
      UPCOMING_START,
      UPCOMING_END,
      session
    );
    await createDraftCycle(seed, projectId, draft, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, done, DONE_START, DONE_END, session);
    const issueA = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue a`, session);
    const issueB = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue b`, session);
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, upcomingId, [issueA.id, issueB.id], session);
    await serverAddCycleFavorite(seed.workspaceSlug, projectId, currentId, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [current, upcoming, draft]);
      await ensureCompletedExpanded(driver, done);

      await test.step("every row shows a completion ring and a date span", async () => {
        for (const name of [current, upcoming, done]) {
          const facts = await driver.cyclesRowFacts(name);
          expect(facts.progress).not.toBe(null);
          expect(facts.dateText).not.toBe(null);
        }
        // Undated drafts show the ring but no date span.
        const draftFacts = await driver.cyclesRowFacts(draft);
        expect(draftFacts.progress).not.toBe(null);
        expect(draftFacts.dateText).toBe(null);
      });

      await test.step("draft and upcoming rows carry work-item counts; others do not", async () => {
        expect((await driver.cyclesRowFacts(upcoming)).workItemCount).toBe("2");
        expect((await driver.cyclesRowFacts(draft)).workItemCount).toBe("0");
        expect((await driver.cyclesRowFacts(current)).workItemCount).toBe(null);
        expect((await driver.cyclesRowFacts(done)).workItemCount).toBe(null);
      });

      await test.step("the active row carries the creator avatar", async () => {
        expect((await driver.cyclesRowFacts(current)).hasCreatorAvatar).toBe(true);
      });

      await test.step("favorites carry a selected marker; others carry an unselected one", async () => {
        const favored = await driver.cyclesRowFacts(current);
        expect(favored.hasFavorite).toBe(true);
        expect(favored.favoriteSelected).toBe(true);
        const plain = await driver.cyclesRowFacts(upcoming);
        expect(plain.hasFavorite).toBe(true);
        expect(plain.favoriteSelected).toBe(false);
        expect(await serverCycleIsFavorite(seed.workspaceSlug, projectId, currentId, session)).toBe(true);
        expect(await serverCycleIsFavorite(seed.workspaceSlug, projectId, upcomingId, session)).toBe(false);
      });

      await test.step("archived rows carry the same markers", async () => {
        const archived = `${tag} archived`;
        const archivedId = await serverCreateCycle(
          seed.workspaceSlug,
          projectId,
          archived,
          DONE_START,
          DONE_END,
          session
        );
        await serverArchiveCycle(seed.workspaceSlug, projectId, archivedId, session);
        await driver.archivesCyclesOpenTabViaLive(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toContain(archived);
        const facts = await driver.cyclesRowFacts(archived);
        expect(facts.progress).not.toBe(null);
        expect(facts.dateText).not.toBe(null);
        const rows = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name)).toContain(archived);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-003"], "cycle rows open a shareable detail address with work items"),
  { tag: specTags(["CYC-003"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 detail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    const issueA = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue a`, session);
    const issueB = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue b`, session);
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [issueA.id, issueB.id], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [name]);

      await test.step("the row links at the detail address", async () => {
        const href = await driver.cyclesRowHref(name);
        expect(href).toContain(`/projects/${projectId}/cycles/${cycleId}`);
      });

      await test.step("clicking the row loads the detail with its work items", async () => {
        await driver.cyclesClickRow(name);
        expect(await driver.cyclesDetailName()).toBe(name);
        await expect
          .poll(async () => (await driver.cyclesDetailIssueNames()).join("\n"), { timeout: 30_000 })
          .toContain(`${tag} issue a`);
        const items = await driver.cyclesDetailIssueNames();
        expect(items.join("\n")).toContain(`${tag} issue b`);
      });

      await test.step("pasting the detail address loads the same cycle", async () => {
        await driver.cyclesOpenDetail(seed.workspaceSlug, projectId, cycleId);
        expect(await driver.cyclesDetailName()).toBe(name);
        await expect
          .poll(async () => (await driver.cyclesDetailIssueNames()).join("\n"), { timeout: 30_000 })
          .toContain(`${tag} issue a`);
        const items = await driver.cyclesDetailIssueNames();
        expect(items.join("\n")).toContain(`${tag} issue b`);
      });

      await test.step("the server agrees on detail and membership", async () => {
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, cycleId, session);
        expect(detail.name).toBe(name);
        const memberIds = await serverCycleIssueIds(seed.workspaceSlug, projectId, cycleId, session);
        expect(memberIds).toContain(issueA.id);
        expect(memberIds).toContain(issueB.id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-004"], "cycle peek opens through the address key and survives reload"),
  { tag: specTags(["CYC-004"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 peek ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const name = `${tag} peeked`;
    const other = `${tag} other`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, other, UPCOMING_START, UPCOMING_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [name, other]);

      await test.step("opening a row peeks it and records the address key", async () => {
        await driver.cyclesOpenPeek(name);
        expect(await driver.cyclesPeekName()).toBe(name);
        expect(await driver.cyclesPeekParam()).toBe(cycleId);
      });

      await test.step("closing the peek clears the key", async () => {
        await driver.cyclesClosePeek();
        expect(await driver.cyclesPeekName()).toBe(null);
        expect(await driver.cyclesPeekParam()).toBe(null);
      });

      await test.step("reload with the key reopens the same panel", async () => {
        await driver.cyclesOpenPeek(name);
        expect(await driver.cyclesPeekParam()).toBe(cycleId);
        await driver.cyclesReload();
        await expect.poll(() => driver.cyclesPeekName(), { timeout: 30_000 }).toBe(name);
        expect(await driver.cyclesPeekParam()).toBe(cycleId);
        await driver.cyclesClosePeek();
      });

      await test.step("a shared peek link opens the same panel", async () => {
        await driver.cyclesOpenPeekLink(seed.workspaceSlug, projectId, cycleId);
        expect(await driver.cyclesPeekName()).toBe(name);
        expect(await driver.cyclesPeekParam()).toBe(cycleId);
      });

      await test.step("the peek body matches the detail read", async () => {
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, cycleId, session);
        expect(detail.name).toBe(name);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-005"], "cycle search expands, filters live, escapes exactly, and survives navigation"),
  { tag: specTags(["CYC-005"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 search ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const alpha = `${tag} alpha`;
    const beta = `${tag} beta`;
    await serverCreateCycle(seed.workspaceSlug, projectId, alpha, UPCOMING_START, UPCOMING_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, beta, UPCOMING_START, UPCOMING_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [alpha, beta]);

      await test.step("the magnifier expands the box", async () => {
        // Live opens unfocused (its input mounts only once open, so the
        // toggle's focus call is a no-op); archived auto-focuses. Typing
        // focuses implicitly, so specs pin expansion only.
        expect(await driver.cyclesSearchExpanded()).toBe(false);
        await driver.cyclesSearchOpen();
        expect(await driver.cyclesSearchExpanded()).toBe(true);
      });

      await test.step("typing filters the list live", async () => {
        await driver.cyclesSearchFill("alpha");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toEqual([alpha]);
      });

      await test.step("Escape clears the text first and collapses on the second press", async () => {
        await driver.cyclesSearchEscape();
        expect(await driver.cyclesSearchText()).toBe("");
        expect(await driver.cyclesSearchExpanded()).toBe(true);
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
        await driver.cyclesSearchEscape();
        expect(await driver.cyclesSearchExpanded()).toBe(false);
      });

      await test.step("the clear button empties and collapses", async () => {
        await driver.cyclesSearchOpen();
        await driver.cyclesSearchFill("beta");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toEqual([beta]);
        await driver.cyclesSearchClear();
        expect(await driver.cyclesSearchText()).toBe("");
        expect(await driver.cyclesSearchExpanded()).toBe(false);
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("clicking away collapses an empty box but keeps a filled one", async () => {
        await driver.cyclesSearchOpen();
        await driver.cyclesClickAway();
        expect(await driver.cyclesSearchExpanded()).toBe(false);
        await driver.cyclesSearchOpen();
        await driver.cyclesSearchFill("alpha");
        await driver.cyclesClickAway();
        expect(await driver.cyclesSearchExpanded()).toBe(true);
        await driver.cyclesSearchClear();
      });

      await test.step("the query survives navigation within the area", async () => {
        await driver.cyclesSearchOpen();
        await driver.cyclesSearchFill("alpha");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toEqual([alpha]);
        await driver.cyclesClickRow(alpha);
        expect(await driver.cyclesDetailName()).toBe(alpha);
        await driver.cyclesGoBack();
        expect(await driver.cyclesSearchText()).toBe("alpha");
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toEqual([alpha]);
      });

      await test.step("searching never touches server state", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles).toHaveLength(2);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-006"], "cycle state filter keeps only matching lifecycles, multi-select"),
  { tag: specTags(["CYC-006"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 status ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const current = `${tag} current`;
    const upcoming = `${tag} upcoming`;
    const draft = `${tag} draft`;
    const done = `${tag} done`;
    await serverCreateCycle(seed.workspaceSlug, projectId, current, CURRENT_START, CURRENT_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, upcoming, UPCOMING_START, UPCOMING_END, session);
    await createDraftCycle(seed, projectId, draft, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, done, DONE_START, DONE_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [current, upcoming, draft]);
      await ensureCompletedExpanded(driver, done);

      await test.step("the filters menu offers the lifecycle states", async () => {
        await driver.cyclesFiltersOpen();
        const sections = await driver.cyclesFilterSections();
        expect(sections.join(" ")).toMatch(/status/i);
        const options = await driver.cyclesFilterOptionNames();
        expect(options.join(" ")).toMatch(/yet to start/i);
        expect(options.join(" ")).toMatch(/draft/i);
      });

      await test.step("selecting one state keeps only matching cycles", async () => {
        await driver.cyclesFilterPick("Status", "Yet to start");
        await driver.cyclesFiltersClose();
        await expect
          .poll(async () => (await driver.cyclesVisibleNames()).join("\n"), { timeout: 30_000 })
          .not.toContain(draft);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).not.toContain(draft);
        expect(names).not.toContain(done);
      });

      await test.step("adding a second state unions the matches", async () => {
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Status", "Draft");
        await driver.cyclesFiltersClose();
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(draft);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).not.toContain(done);
      });

      await test.step("deselecting restores the hidden cycles", async () => {
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Status", "Yet to start");
        await driver.cyclesFilterPick("Status", "Draft");
        await driver.cyclesFiltersClose();
        await expect
          .poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 })
          .toContain(done);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(current);
        expect(names).toContain(upcoming);
        expect(names).toContain(draft);
      });

      await test.step("filtering never touches server state", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles).toHaveLength(4);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-007"], "cycle date-window filters keep only overlapping cycles"),
  { tag: specTags(["CYC-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 dates ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const current = `${tag} current`;
    const upcoming = `${tag} upcoming`;
    const done = `${tag} done`;
    const draft = `${tag} draft`;
    await serverCreateCycle(seed.workspaceSlug, projectId, current, CURRENT_START, CURRENT_END_SHORT, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, upcoming, UPCOMING_START, UPCOMING_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, done, DONE_START, DONE_END, session);
    await createDraftCycle(seed, projectId, draft, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [current, upcoming, draft]);
      await ensureCompletedExpanded(driver, done);

      await test.step("the filters menu offers start and end windows", async () => {
        await driver.cyclesFiltersOpen();
        const sections = await driver.cyclesFilterSections();
        expect(sections.join(" ")).toMatch(/start date/i);
        const options = await driver.cyclesFilterOptionNames();
        expect(options.join(" ")).toMatch(/1 week from now/i);
        await driver.cyclesFiltersClose();
      });

      await test.step("a start window keeps only cycles starting inside it", async () => {
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Start date", "1 week from now");
        await driver.cyclesFiltersClose();
        await expect
          .poll(async () => (await driver.cyclesVisibleNames()).join("\n"), { timeout: 30_000 })
          .not.toContain(done);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).not.toContain(done);
        expect(names).not.toContain(draft);
        await driver.cyclesFiltersClearAll();
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(done);
      });

      await test.step("an end window keeps only cycles ending inside it", async () => {
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Due date", "1 month from now");
        await driver.cyclesFiltersClose();
        await expect
          .poll(async () => (await driver.cyclesVisibleNames()).join("\n"), { timeout: 30_000 })
          .not.toContain(done);
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).not.toContain(done);
        expect(names).not.toContain(draft);
        await driver.cyclesFiltersClearAll();
      });

      await test.step("filtering never touches server state", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles).toHaveLength(4);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-008"], "applied cycle filters render as removable chips with clear-all"),
  { tag: specTags(["CYC-008"]) },
  async ({ driver, seed }) => {
    const tag = `NF249 chips ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N249"),
      { cycleView: true },
      session
    );
    const current = `${tag} current`;
    const upcoming = `${tag} upcoming`;
    const done = `${tag} done`;
    await serverCreateCycle(seed.workspaceSlug, projectId, current, CURRENT_START, CURRENT_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, upcoming, UPCOMING_START, UPCOMING_END, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, done, DONE_START, DONE_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [current, upcoming]);
      await ensureCompletedExpanded(driver, done);

      await test.step("no chips show before any filter applies", async () => {
        expect(await driver.cyclesFilterChipTexts()).toEqual([]);
        expect(await driver.cyclesFiltersActive()).toBe(false);
      });

      await test.step("each applied dimension renders its own chip", async () => {
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Status", "Yet to start");
        await driver.cyclesFilterPick("Start date", "1 week from now");
        await driver.cyclesFiltersClose();
        await expect.poll(() => driver.cyclesFilterChipTexts(), { timeout: 30_000 }).toHaveLength(2);
        expect(await driver.cyclesFiltersActive()).toBe(true);
      });

      await test.step("removing one chip restores its dimension only", async () => {
        const before = await driver.cyclesFilterChipTexts();
        const statusChip = before.find((entry) => /status/i.test(entry)) ?? "";
        expect(statusChip).not.toBe("");
        await driver.cyclesFilterRemoveChip(statusChip);
        await expect.poll(() => driver.cyclesFilterChipTexts(), { timeout: 30_000 }).toHaveLength(1);
        const after = await driver.cyclesFilterChipTexts();
        expect(after[0]).toMatch(/start date/i);
        // The start window still narrows the list to the upcoming cycle.
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).not.toContain(done);
      });

      await test.step("clear-all resets filters together", async () => {
        await driver.cyclesFiltersClearAll();
        await expect.poll(() => driver.cyclesFilterChipTexts(), { timeout: 30_000 }).toEqual([]);
        await expect.poll(() => driver.cyclesVisibleNames(), { timeout: 30_000 }).toContain(done);
        expect(await driver.cyclesFiltersActive()).toBe(false);
      });

      await test.step("clear-all leaves an active search in place", async () => {
        // Clear-all resets filter dimensions; the search box keeps its text
        // (it has its own clear) and keeps filtering the list.
        await driver.cyclesFiltersOpen();
        await driver.cyclesFilterPick("Status", "Yet to start");
        await driver.cyclesFiltersClose();
        await driver.cyclesSearchOpen();
        await driver.cyclesSearchFill("upcoming");
        await driver.cyclesFiltersClearAll();
        await expect.poll(() => driver.cyclesFilterChipTexts(), { timeout: 30_000 }).toEqual([]);
        expect(await driver.cyclesSearchText()).toBe("upcoming");
        const names = await driver.cyclesVisibleNames();
        expect(names).toContain(upcoming);
        expect(names).toContain(current);
        expect(names).not.toContain(done);
      });

      await test.step("filtering never touches server state", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles).toHaveLength(3);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
