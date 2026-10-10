// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-224): archived cycles — plain list with an
// address-driven side peek, exact search-box semantics, date filters with
// an applied-chip row, empty/no-match/skeleton states, the completed-only
// archive dialog, and restore from the row menu.
//
// Fresh archived loads hold the skeleton forever (bug NEWFRONT-231: the
// archives routes mount outside the project layout, so the cycle fetched
// flag is never set). Intended-behavior scenarios therefore open the tab
// through the live screen (which sets the flag) plus client-side hops;
// the bug: scenario pins the fresh-load behavior itself. Archive/restore
// backend failures also report success (bug NEWFRONT-244: the store
// swallows the error); those bug: scenarios pin the success-on-failure
// behavior and the rows record the intended error confirmations.
// Rows: ARCH-014–ARCH-019.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverArchivedCycleDetail,
  serverArchivedCycles,
  serverArchiveCycle,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateProjectWithFlags,
  serverDeleteCycle,
  serverEnsureProjectGuest,
  serverPatchCycle,
  serverProjectCycles,
  serverRequestStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Cycle dates that read CURRENT on the seeded stack (today is 2026-10-09). */
const CURRENT_START = "2026-09-01";
const CURRENT_END = "2026-12-31";
/** Cycle dates that read COMPLETED (the archive gate). */
const DONE_START = "2026-01-01";
const DONE_END = "2026-02-01";
/** Cycle dates that read UPCOMING (a non-completed cycle that still renders a row). */
const UPCOMING_START = "2026-11-01";
const UPCOMING_END = "2026-12-31";

/** Project role numbers (backend ROLE enum): member edits, guest only reads. */
const ROLE_MEMBER = 15;

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/**
 * Create a cycle reading COMPLETED: the create call takes current dates
 * (the server rejects completed-looking creates), then a patch backdates
 * it into the archive gate. Resolves with the cycle id.
 */
async function createCompletedCycle(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  session: string
): Promise<string> {
  const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
  await serverPatchCycle(seed.workspaceSlug, projectId, id, { start_date: DONE_START, end_date: DONE_END }, session);
  return id;
}

/** A completed cycle archived through the API; resolves with the cycle id. */
async function createArchivedCycle(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  session: string
): Promise<string> {
  const id = await createCompletedCycle(seed, projectId, name, session);
  await serverArchiveCycle(seed.workspaceSlug, projectId, id, session);
  return id;
}

/**
 * One live cycle so the live screen (the waypoint into the tab) settles.
 * Pure scaffolding for NEWFRONT-231: fresh archived loads never settle.
 */
async function createLiveAnchor(seed: ParitySeedFacts, projectId: string, tag: string, session: string): Promise<void> {
  await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} live anchor`, CURRENT_START, CURRENT_END, session);
}

/**
 * Open the archived tab through the live screen until `wanted` names
 * render, retrying the whole navigation once: the oracle dev server
 * stalls whole renders under shared-stack load, and a stalled first
 * pass must not fail an assertion. Genuinely absent rows still fail.
 */
async function openTabViaLiveSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  wanted: string[]
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    try {
      await driver.archivesCyclesOpenTabViaLive(workspaceSlug, projectId);
      await expect
        .poll(
          async () => {
            const names = await driver.archivesCyclesVisibleNames();
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

/** Current path without a trailing slash (client-side hops add one). */
async function currentPathClean(driver: ParityDriver): Promise<string> {
  return (await driver.currentPath()).replace(/\/$/, "");
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

test(
  specTitle(["ARCH-014", "ARCH-017"], "bug: NEWFRONT-231 fresh archived-cycles loads hold the skeleton forever"),
  { tag: specTags(["ARCH-014", "ARCH-017"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 skeleton ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const emptyProjectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} empty project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} archived`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a filled project shows the skeleton, never rows", async () => {
        await driver.archivesCyclesOpenTabRaw(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesCyclesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
        // The fetch succeeds but nothing renders from it.
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 10_000 }).toEqual([]);
        expect(await driver.archivesCyclesEmptyHeading()).toBe(null);
        expect(await driver.archivesCyclesNoMatchHint()).toBe(null);
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.id)).toContain(cycleId);
      });

      await test.step("a zero-archived project shows the skeleton, never the empty state", async () => {
        await driver.archivesCyclesOpenTabRaw(seed.workspaceSlug, emptyProjectId);
        await expect.poll(() => driver.archivesCyclesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.archivesCyclesEmptyHeading()).toBe(null);
        const archived = await serverArchivedCycles(seed.workspaceSlug, emptyProjectId, session);
        expect(archived).toEqual([]);
      });

      await test.step("a shared peek link keeps the param but opens no peek", async () => {
        await driver.archivesCyclesOpenPeekLink(seed.workspaceSlug, projectId, cycleId);
        await expect.poll(() => driver.archivesCyclesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.archivesCyclesPeekParam()).toBe(cycleId);
        expect(await driver.archivesCyclesPeekName()).toBe(null);
      });

      await test.step("reloading drops the open peek back to the skeleton", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [name]);
        await driver.archivesCyclesOpenPeek(name);
        expect(await driver.archivesCyclesPeekName()).toBe(name);
        await driver.archivesCyclesReload();
        await expect.poll(() => driver.archivesCyclesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.archivesCyclesPeekParam()).toBe(cycleId);
        expect(await driver.archivesCyclesPeekName()).toBe(null);
        expect(await driver.archivesCyclesVisibleNames()).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, emptyProjectId, session);
    }
  }
);

test(
  specTitle(["ARCH-014"], "archived cycles render as a flat list while live cycles group"),
  { tag: specTags(["ARCH-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 flat ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const archivedA = `${tag} archived a`;
    const archivedB = `${tag} archived b`;
    const liveUpcoming = `${tag} live upcoming`;
    const liveDone = `${tag} live done`;
    await createArchivedCycle(seed, projectId, archivedA, session);
    await createArchivedCycle(seed, projectId, archivedB, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, liveUpcoming, UPCOMING_START, UPCOMING_END, session);
    await createCompletedCycle(seed, projectId, liveDone, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the archived tab lists only archived cycles with no grouping", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [archivedA, archivedB]);
        expect(await driver.archivesCyclesVisibleNames()).toEqual(expect.arrayContaining([archivedA, archivedB]));
        expect(await driver.archivesCyclesVisibleNames()).toHaveLength(2);
        expect(await driver.archivesCyclesGroupHeadings()).toEqual([]);
      });

      await test.step("the live screen groups instead", async () => {
        await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
        const headings = await expect
          .poll(() => driver.archivesCyclesGroupHeadings(), { timeout: 60_000 })
          .not.toEqual([])
          .then(() => driver.archivesCyclesGroupHeadings());
        expect(headings.join(" ")).toMatch(/upcoming/i);
        expect(headings.join(" ")).toMatch(/completed/i);
        expect(await driver.archivesCyclesVisibleNames()).toContain(liveUpcoming);
      });

      await test.step("the server agrees on archived vs live membership", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.name).sort()).toEqual([archivedA, archivedB].sort());
        expect(archived.every((row) => row.archivedAt !== null)).toBe(true);
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.name).sort()).toEqual([liveUpcoming, liveDone].sort());
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-014"], "archived peek opens from a row and clears from the address"),
  { tag: specTags(["ARCH-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 peek ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} peeked`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [name]);

      await test.step("opening a row peeks it and records the address param", async () => {
        await driver.archivesCyclesOpenPeek(name);
        expect(await driver.archivesCyclesPeekName()).toBe(name);
        expect(await driver.archivesCyclesPeekParam()).toBe(cycleId);
      });

      await test.step("closing the peek clears the param", async () => {
        await driver.archivesCyclesClosePeek();
        expect(await driver.archivesCyclesPeekName()).toBe(null);
        expect(await driver.archivesCyclesPeekParam()).toBe(null);
      });

      // Reload and shared-link reopening are pinned by the bug: scenario
      // until NEWFRONT-231 lands (fresh loads hold the skeleton).

      await test.step("the peek body matches the archived detail read", async () => {
        const detail = await serverArchivedCycleDetail(seed.workspaceSlug, projectId, cycleId, session);
        expect(detail.name).toBe(name);
        expect(detail.archivedAt).not.toBe(null);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-015"], "archived-cycle search expands, filters live, and collapses exactly"),
  { tag: specTags(["ARCH-015"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 search ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const alpha = `${tag} alpha`;
    const beta = `${tag} beta`;
    await createArchivedCycle(seed, projectId, alpha, session);
    await createArchivedCycle(seed, projectId, beta, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [alpha, beta]);

      await test.step("the magnifier expands the box and focuses it", async () => {
        expect(await driver.archivesCyclesSearchExpanded()).toBe(false);
        await driver.archivesCyclesSearchOpen();
        expect(await driver.archivesCyclesSearchExpanded()).toBe(true);
        expect(await driver.archivesCyclesSearchFocused()).toBe(true);
      });

      await test.step("typing filters the list live", async () => {
        await driver.archivesCyclesSearchFill("alpha");
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toEqual([alpha]);
      });

      await test.step("Escape clears the text first and collapses on the second press", async () => {
        await driver.archivesCyclesSearchEscape();
        expect(await driver.archivesCyclesSearchText()).toBe("");
        expect(await driver.archivesCyclesSearchExpanded()).toBe(true);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
        await driver.archivesCyclesSearchEscape();
        expect(await driver.archivesCyclesSearchExpanded()).toBe(false);
      });

      await test.step("the clear button empties and collapses", async () => {
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesSearchFill("beta");
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toEqual([beta]);
        await driver.archivesCyclesSearchClear();
        expect(await driver.archivesCyclesSearchText()).toBe("");
        expect(await driver.archivesCyclesSearchExpanded()).toBe(false);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("clicking away collapses an empty box but keeps a filled one", async () => {
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesClickAway();
        expect(await driver.archivesCyclesSearchExpanded()).toBe(false);
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesSearchFill("alpha");
        await driver.archivesCyclesClickAway();
        expect(await driver.archivesCyclesSearchExpanded()).toBe(true);
        await driver.archivesCyclesSearchClear();
      });

      await test.step("searching never touches server state", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toHaveLength(2);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-016"], "archived-cycle date filters narrow the list and chips manage them"),
  { tag: specTags(["ARCH-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 filters ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const first = `${tag} first`;
    const second = `${tag} second`;
    await createArchivedCycle(seed, projectId, first, session);
    await createArchivedCycle(seed, projectId, second, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [first, second]);

      await test.step("no filters are applied on entry", async () => {
        expect(await driver.archivesCyclesFilterChipTexts()).toEqual([]);
        expect(await driver.archivesCyclesFiltersActive()).toBe(false);
      });

      await test.step("the menu offers start and due dates, not status", async () => {
        await driver.archivesCyclesFiltersOpen();
        const sections = await driver.archivesCyclesFilterSections();
        expect(sections).toEqual(expect.arrayContaining(["Start date", "Due date"]));
        expect(sections.join(" ")).not.toMatch(/status/i);
        // A future-relative start option hides every past-start fixture.
        expect(await driver.archivesCyclesFilterOptionNames()).toContain("1 week from now");
        await driver.archivesCyclesFilterPick("Start date", "1 week from now");
      });

      await test.step("applying a filter chips it, marks the menu, and narrows the list", async () => {
        await expect.poll(() => driver.archivesCyclesFilterChipTexts(), { timeout: 30_000 }).not.toEqual([]);
        await expect.poll(() => driver.archivesCyclesFiltersActive(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toEqual([]);
        await driver.archivesCyclesFiltersClose();
      });

      await test.step("removing the chip restores the list", async () => {
        const chips = await driver.archivesCyclesFilterChipTexts();
        expect(chips).toHaveLength(1);
        await driver.archivesCyclesFilterRemoveChip(chips[0] as string);
        await expect.poll(() => driver.archivesCyclesFilterChipTexts(), { timeout: 30_000 }).toEqual([]);
        expect(await driver.archivesCyclesFiltersActive()).toBe(false);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("clear-all drops every chip at once", async () => {
        await driver.archivesCyclesFiltersOpen();
        await driver.archivesCyclesFilterPick("Start date", "1 week from now");
        await driver.archivesCyclesFilterPick("Due date", "1 week from now");
        await expect.poll(() => driver.archivesCyclesFilterChipTexts(), { timeout: 30_000 }).toHaveLength(2);
        await driver.archivesCyclesFiltersClose();
        await driver.archivesCyclesFiltersClearAll();
        await expect.poll(() => driver.archivesCyclesFilterChipTexts(), { timeout: 30_000 }).toEqual([]);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("filtering never touches server state", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toHaveLength(2);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-017"], "empty archives explain themselves after the skeleton loads"),
  { tag: specTags(["ARCH-017"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 empty ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a skeleton shows while the list fetches", async () => {
        expect(await driver.archivesCyclesSkeletonShownOnSlowFetchViaLive(seed.workspaceSlug, projectId)).toBe(true);
      });

      await test.step("zero archived cycles render the illustrated empty state", async () => {
        expect(await driver.archivesCyclesEmptyHeading()).not.toBe(null);
        expect(await driver.archivesCyclesVisibleNames()).toEqual([]);
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-017"], "filters-hide-all and search-hides-all show distinct no-match hints"),
  { tag: specTags(["ARCH-017"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 nomatch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const first = `${tag} first`;
    const second = `${tag} second`;
    await createArchivedCycle(seed, projectId, first, session);
    await createArchivedCycle(seed, projectId, second, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [first, second]);

      await test.step("an excluding filter shows the filters no-match hint", async () => {
        await driver.archivesCyclesFiltersOpen();
        await driver.archivesCyclesFilterPick("Start date", "1 week from now");
        await driver.archivesCyclesFiltersClose();
        await expect.poll(() => driver.archivesCyclesNoMatchHint(), { timeout: 30_000 }).not.toBe(null);
        const filtersHint = await driver.archivesCyclesNoMatchHint();
        expect(filtersHint as string).toMatch(/filter/i);
        await driver.archivesCyclesFiltersClearAll();
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("a matching-nothing search shows the search no-match hint", async () => {
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesSearchFill("zzz-no-such-cycle");
        await expect.poll(() => driver.archivesCyclesNoMatchHint(), { timeout: 30_000 }).not.toBe(null);
        const searchHint = await driver.archivesCyclesNoMatchHint();
        expect(searchHint as string).toMatch(/search/i);
        await driver.archivesCyclesSearchClear();
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("the two hints differ and server state is untouched", async () => {
        await driver.archivesCyclesFiltersOpen();
        await driver.archivesCyclesFilterPick("Start date", "1 week from now");
        await driver.archivesCyclesFiltersClose();
        const filtersHint = await expect
          .poll(() => driver.archivesCyclesNoMatchHint(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.archivesCyclesNoMatchHint());
        await driver.archivesCyclesFiltersClearAll();
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesSearchFill("zzz-no-such-cycle");
        const searchHint = await expect
          .poll(() => driver.archivesCyclesNoMatchHint(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.archivesCyclesNoMatchHint());
        expect(filtersHint).not.toBe(searchHint);
        await driver.archivesCyclesSearchClear();
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toHaveLength(2);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "archiving a completed cycle confirms and lands on live cycles"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 arch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} to archive`;
    const cycleId = await createCompletedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");
      await expect
        .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(name), { timeout: 60_000 })
        .toBe(true);

      await test.step("the live row offers an enabled archive entry", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        const entries = await driver.archivesCyclesMenuEntries();
        const archive = entries.find((entry) => entry.title === "Archive");
        expect(archive).toBeDefined();
        expect(archive?.disabled).toBe(false);
        expect(archive?.description).toBe(null);
      });

      await test.step("the dialog names the cycle and notes it can be restored", async () => {
        await driver.archivesCyclesMenuPick("Archive");
        const dialog = await driver.archivesCyclesArchiveDialogText();
        expect(dialog).not.toBe(null);
        expect(dialog?.heading ?? "").toContain(name);
        expect(dialog?.body ?? "").toMatch(/restor/i);
      });

      await test.step("confirming archives, toasts success, and lands on live cycles", async () => {
        await driver.archivesCyclesArchiveDialogConfirm();
        expect(await toastText(driver)).toMatch(/archive success/i);
        await expect
          .poll(() => currentPathClean(driver), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
      });

      await test.step("the server moved the cycle to the archives", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.id)).toContain(cycleId);
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.id)).not.toContain(cycleId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "archive entry gates on completion and hides for archived cycles"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const upcoming = `${tag} upcoming`;
    const archivedName = `${tag} archived`;
    await serverCreateCycle(seed.workspaceSlug, projectId, upcoming, UPCOMING_START, UPCOMING_END, session);
    await createArchivedCycle(seed, projectId, archivedName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("an upcoming cycle's archive entry is disabled with an explanation", async () => {
        await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
        await expect
          .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(upcoming), { timeout: 60_000 })
          .toBe(true);
        await driver.archivesCyclesOpenRowMenu(upcoming);
        const entries = await driver.archivesCyclesMenuEntries();
        const archive = entries.find((entry) => entry.title === "Archive");
        expect(archive).toBeDefined();
        expect(archive?.disabled).toBe(true);
        expect(archive?.description ?? "").toMatch(/completed/i);
      });

      await test.step("an archived cycle offers no archive entry", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [archivedName]);
        await driver.archivesCyclesOpenRowMenu(archivedName);
        const entries = await driver.archivesCyclesMenuEntries();
        expect(entries.map((entry) => entry.title)).not.toContain("Archive");
      });

      await test.step("the server kept both cycles where they were", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.name)).toEqual([archivedName]);
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.name)).toEqual([upcoming]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "cancelling the archive dialog changes nothing"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 cancel ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} kept live`;
    const cycleId = await createCompletedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");
      await expect
        .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(name), { timeout: 60_000 })
        .toBe(true);

      await driver.archivesCyclesOpenRowMenu(name);
      await driver.archivesCyclesMenuPick("Archive");
      expect(await driver.archivesCyclesArchiveDialogText()).not.toBe(null);
      await driver.archivesCyclesArchiveDialogCancel();
      expect(await driver.archivesCyclesArchiveDialogText()).toBe(null);
      expect(await currentPathClean(driver)).toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);

      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).not.toContain(cycleId);
      const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
      expect(live.map((row) => row.id)).toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "bug: NEWFRONT-244 archive failure still reports success and navigates away"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 archfail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} doomed`;
    const cycleId = await createCompletedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");
      await expect
        .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(name), { timeout: 60_000 })
        .toBe(true);

      // Delete under the open dialog so the confirm's POST 404s. The store
      // swallows the failure (NEWFRONT-244), so the UI takes the success
      // branch exactly as if the archive had worked. Intended: an error
      // confirmation with no navigation.
      await driver.archivesCyclesOpenRowMenu(name);
      await driver.archivesCyclesMenuPick("Archive");
      expect(await driver.archivesCyclesArchiveDialogText()).not.toBe(null);
      await serverDeleteCycle(seed.workspaceSlug, projectId, cycleId, session);
      await driver.archivesCyclesArchiveDialogConfirm();

      expect(await toastText(driver)).toMatch(/archive success/i);
      expect(await currentPathClean(driver)).toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
      const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
      expect(live.map((row) => row.id)).not.toContain(cycleId);
      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).not.toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "project members see the archive entry and can archive"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 member ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    if (!seed.mentionMember) throw new Error("[parity] seed has no member identity; rerun parity-up.sh.");
    const member = seed.mentionMember;
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: ROLE_MEMBER }], session);
    const name = `${tag} member archives`;
    const cycleId = await createCompletedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");
      await expect
        .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(name), { timeout: 60_000 })
        .toBe(true);

      await driver.archivesCyclesOpenRowMenu(name);
      const entries = await driver.archivesCyclesMenuEntries();
      const archive = entries.find((entry) => entry.title === "Archive");
      expect(archive).toBeDefined();
      expect(archive?.disabled).toBe(false);

      const memberSession = await signInSession(member.email, member.password);
      const res = await serverRequestStatus(
        "POST",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
        memberSession
      );
      expect(res.status).toBe(200);
      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-018"], "guests see no archive entry and the server refuses them"),
  { tag: specTags(["ARCH-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 guestarch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const guest = requireGuest(seed);
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
    const name = `${tag} guest cannot archive`;
    const cycleId = await createCompletedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");
      await expect
        .poll(async () => (await driver.archivesCyclesVisibleNames()).includes(name), { timeout: 60_000 })
        .toBe(true);

      await driver.archivesCyclesOpenRowMenu(name);
      const titles = (await driver.archivesCyclesMenuEntries()).map((entry) => entry.title);
      expect(titles).toContain("Copy link");
      expect(titles).toContain("Open in new tab");
      expect(titles).not.toContain("Archive");
      expect(titles).not.toContain("Restore");
      expect(titles).not.toContain("Edit");
      expect(titles).not.toContain("Delete");

      const guestSession = await signInSession(guest.email, guest.password);
      const res = await serverRequestStatus(
        "POST",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
        guestSession
      );
      expect(res.status).toBe(403);
      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).not.toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-019"], "restoring an archived cycle keeps the user on the cycles tab"),
  { tag: specTags(["ARCH-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 restore ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} to restore`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [name]);

      await test.step("the archived row offers restore next to link actions, without edit/delete", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        const entries = await driver.archivesCyclesMenuEntries();
        const titles = entries.map((entry) => entry.title);
        expect(titles).toContain("Restore");
        expect(titles).toContain("Copy link");
        expect(titles).toContain("Open in new tab");
        expect(titles).not.toContain("Edit");
        expect(titles).not.toContain("Delete");
        expect(entries.find((entry) => entry.title === "Restore")?.disabled).toBe(false);
      });

      await test.step("restoring toasts success and stays on the cycles tab", async () => {
        await driver.archivesCyclesMenuPick("Restore");
        expect(await toastText(driver)).toMatch(/restor/i);
        await expect
          .poll(() => currentPathClean(driver), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/projects/${projectId}/archives/cycles`);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).not.toContain(name);
      });

      await test.step("the server moved the cycle back to live", async () => {
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.id)).not.toContain(cycleId);
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.id)).toContain(cycleId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-019"], "bug: NEWFRONT-244 restore failure still reports success and the row stays"),
  { tag: specTags(["ARCH-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 restfail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const name = `${tag} stuck`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [name]);

      // Delete under the stale row so the restore's DELETE 404s. The store
      // swallows the failure (NEWFRONT-244), so the UI toasts success while
      // the row (whose archived flag never cleared) stays rendered.
      // Intended: an error confirmation instead of the success toast.
      await driver.archivesCyclesOpenRowMenu(name);
      await serverDeleteCycle(seed.workspaceSlug, projectId, cycleId, session);
      await driver.archivesCyclesMenuPick("Restore");

      expect(await toastText(driver)).toMatch(/cycle restored/i);
      await expect
        .poll(() => currentPathClean(driver), { timeout: 30_000 })
        .toBe(`/${seed.workspaceSlug}/projects/${projectId}/archives/cycles`);
      await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toContain(name);
      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).not.toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-019"], "guests reach no restore entry and the server refuses them"),
  { tag: specTags(["ARCH-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF224 guestrest ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N224"),
      { cycleView: true },
      session
    );
    const guest = requireGuest(seed);
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
    const name = `${tag} guest cannot restore`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenTabRaw(seed.workspaceSlug, projectId);

      // Fresh loads hold the skeleton for every role (NEWFRONT-231); the
      // guest additionally cannot read the list at all, so no row — and
      // no restore entry — ever renders for them.
      await expect.poll(() => driver.archivesCyclesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      expect(await driver.archivesCyclesVisibleNames()).toEqual([]);

      const guestSession = await signInSession(guest.email, guest.password);
      const listRes = await serverRequestStatus(
        "GET",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/archived-cycles/`,
        guestSession
      );
      expect(listRes.status).toBe(403);
      const restoreRes = await serverRequestStatus(
        "DELETE",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
        guestSession
      );
      expect(restoreRes.status).toBe(403);
      const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
      expect(archived.map((row) => row.id)).toContain(cycleId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
