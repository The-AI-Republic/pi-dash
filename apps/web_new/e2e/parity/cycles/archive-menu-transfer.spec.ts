// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-252): cycles archive gating, restore, the
// archived list from the cycles side, the per-row action menu matrix,
// personal favorites, palette cycle commands, transfer to an open cycle,
// and copy-link / open-in-new-tab from the row menu and the palette.
// Rows: CYC-025–CYC-031 plus CYC-046.
//
// The archived surface is shared with archives/cycles.spec.ts (ARCH-014–
// 019, read-only): these scenarios assert the cycles-area acceptance on it
// without repeating that suite — the archive gate on draft/current rows,
// restore landing back in the live list, and a thin search/filter/zero-
// state pass. Transfer overlaps ISS-224 (prompt, move, snapshot freeze),
// so CYC-031 leans on target eligibility, search narrowing, the no-open-
// cycle zero-state, and both-sides refresh through the UI. Palette overlap
// with SHELL-091 (offer + add persists) is handled the same way: CYC-030
// proves the label flip in both directions plus the archived absence.
// Restore failures pin bug NEWFRONT-244 (the store reports success); fresh
// archived loads hold the skeleton per bug NEWFRONT-231, so the tab is
// always entered through the live screen.
import { test, expect } from "../fixtures";
import {
  cyclesUnfavoriteCycle,
  deleteFavorite,
  listFavorites,
  parityApiBase,
  parityProjectIdentifier,
  serverArchivedCycles,
  serverArchiveCycle,
  serverAttachCycleIssues,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCycleIsFavorite,
  serverCycleProgress,
  serverDeleteCycle,
  serverEnsureProjectGuest,
  serverIssue,
  serverPatchCycle,
  serverProjectCycles,
  serverRequestStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Cycle dates that read CURRENT on the seeded stack (today is 2026-10-10). */
const CURRENT_START = "2026-09-01";
const CURRENT_END = "2026-12-31";
/** Cycle dates that read COMPLETED (the archive gate). */
const DONE_START = "2026-01-01";
const DONE_END = "2026-02-01";
/** Cycle dates that read UPCOMING. */
const UPCOMING_START = "2026-11-01";
const UPCOMING_END = "2026-12-31";

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

/**
 * Create an undated cycle reading DRAFT: create dated (the create call
 * needs a range), then clear the dates. Resolves with the cycle id.
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
 * One live cycle so the live screen (the waypoint into the archived tab)
 * settles. Pure scaffolding for NEWFRONT-231: fresh archived loads never
 * settle.
 */
async function createLiveAnchor(seed: ParitySeedFacts, projectId: string, tag: string, session: string): Promise<void> {
  await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} live anchor`, CURRENT_START, CURRENT_END, session);
}

/**
 * Open the live cycles list until `wanted` names render, opening the named
 * collapsed groups along the way and retrying once: the oracle dev server
 * stalls whole renders under shared-stack load, and a stalled first pass
 * must not fail an assertion. Genuinely absent rows still fail. Groups
 * open by explicit name (never "every heading"): opening toggles, so an
 * already-open group would close again.
 */
async function openLiveSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  wanted: string[],
  groups: string[] = ["Completed"]
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    try {
      await driver.archivesCyclesOpenLive(workspaceSlug, projectId);
      for (const section of groups) {
        try {
          await driver.archivesCyclesLiveGroupOpen(section);
        } catch {
          // A group that will not open holds no wanted rows either way.
        }
      }
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

/**
 * Open the archived tab through the live screen until `wanted` names
 * render, retrying the whole navigation once (same stall rationale as
 * openLiveSettled).
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

/** Menu entry titles in display order for the row menu of `name`. */
async function menuTitles(driver: ParityDriver, name: string): Promise<string[]> {
  await driver.archivesCyclesOpenRowMenu(name);
  return (await driver.archivesCyclesMenuEntries()).map((entry) => entry.title);
}

test(
  specTitle(["CYC-025"], "draft and current cycles gate the archive entry with an explanation"),
  { tag: specTags(["CYC-025"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const draft = `${tag} draft`;
    const current = `${tag} current`;
    const done = `${tag} done`;
    await createDraftCycle(seed, projectId, draft, session);
    await serverCreateCycle(seed.workspaceSlug, projectId, current, CURRENT_START, CURRENT_END, session);
    const doneId = await createCompletedCycle(seed, projectId, done, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [draft, current], []);

      await test.step("a draft cycle's archive entry is disabled with an explanation", async () => {
        await driver.archivesCyclesOpenRowMenu(draft);
        const entries = await driver.archivesCyclesMenuEntries();
        const archive = entries.find((entry) => entry.title === "Archive");
        expect(archive).toBeDefined();
        expect(archive?.disabled).toBe(true);
        expect(archive?.description ?? "").toMatch(/completed/i);
      });

      await test.step("a current cycle's archive entry is disabled with an explanation", async () => {
        await driver.archivesCyclesOpenRowMenu(current);
        const entries = await driver.archivesCyclesMenuEntries();
        const archive = entries.find((entry) => entry.title === "Archive");
        expect(archive).toBeDefined();
        expect(archive?.disabled).toBe(true);
        expect(archive?.description ?? "").toMatch(/completed/i);
      });

      await test.step("a completed cycle's archive entry is enabled with no hint", async () => {
        await openLiveSettled(driver, seed.workspaceSlug, projectId, [done]);
        await driver.archivesCyclesOpenRowMenu(done);
        const entries = await driver.archivesCyclesMenuEntries();
        const archive = entries.find((entry) => entry.title === "Archive");
        expect(archive).toBeDefined();
        expect(archive?.disabled).toBe(false);
        expect(archive?.description).toBe(null);
      });

      await test.step("the server kept every cycle live", async () => {
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.id)).toContain(doneId);
        expect(live).toHaveLength(3);
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-026"], "restoring returns the cycle to the live list through the archives list"),
  { tag: specTags(["CYC-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 restore ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} to restore`;
    const cycleId = await createArchivedCycle(seed, projectId, name, session);
    await createLiveAnchor(seed, projectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [name]);

      await test.step("restoring toasts success and lands on the archives list", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        await driver.archivesCyclesMenuPick("Restore");
        expect(await toastText(driver)).toMatch(/restor/i);
        await expect
          .poll(() => currentPathClean(driver), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/projects/${projectId}/archives/cycles`);
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).not.toContain(name);
      });

      await test.step("the cycle reads live again, in the UI and on the server", async () => {
        await openLiveSettled(driver, seed.workspaceSlug, projectId, [name]);
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
  specTitle(["CYC-026"], "bug: NEWFRONT-244 a failed restore still reports success and the cycle stays archived"),
  { tag: specTags(["CYC-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 restfail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
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
      // swallows the failure (NEWFRONT-244), so the UI takes the success
      // branch exactly as if the restore had worked. Intended: an error
      // confirmation instead of the success toast.
      await driver.archivesCyclesOpenRowMenu(name);
      await serverDeleteCycle(seed.workspaceSlug, projectId, cycleId, session);
      await driver.archivesCyclesMenuPick("Restore");

      expect(await toastText(driver)).toMatch(/restor/i);
      await expect
        .poll(() => currentPathClean(driver), { timeout: 30_000 })
        .toBe(`/${seed.workspaceSlug}/projects/${projectId}/archives/cycles`);
      // The row (whose archived flag never cleared) stays rendered, and the
      // cycle never reaches the live list.
      await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toContain(name);
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
  specTitle(["CYC-027"], "archived rows render with working search, filters, and an archives zero-state"),
  { tag: specTags(["CYC-027"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 archlist ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const emptyProjectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} empty project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const alpha = `${tag} alpha`;
    const beta = `${tag} beta`;
    await createArchivedCycle(seed, projectId, alpha, session);
    await createArchivedCycle(seed, projectId, beta, session);
    await createLiveAnchor(seed, projectId, tag, session);
    await createLiveAnchor(seed, emptyProjectId, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("archived rows render", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [alpha, beta]);
        expect(await driver.archivesCyclesVisibleNames()).toHaveLength(2);
      });

      await test.step("search narrows the archived rows", async () => {
        await driver.archivesCyclesSearchOpen();
        await driver.archivesCyclesSearchFill("alpha");
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toEqual([alpha]);
        await driver.archivesCyclesSearchClear();
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("a date filter narrows the archived rows and chips it", async () => {
        await driver.archivesCyclesFiltersOpen();
        await driver.archivesCyclesFilterPick("Start date", "1 week from now");
        await driver.archivesCyclesFiltersClose();
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toEqual([]);
        expect(await driver.archivesCyclesFilterChipTexts()).not.toEqual([]);
        await driver.archivesCyclesFiltersClearAll();
        await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("with none archived the zero-state names the archives", async () => {
        await driver.archivesCyclesOpenTabViaLive(seed.workspaceSlug, emptyProjectId);
        await expect.poll(() => driver.archivesCyclesEmptyHeading(), { timeout: 30_000 }).not.toBe(null);
        expect(await driver.archivesCyclesVisibleNames()).toEqual([]);
        expect((await driver.cyclesArchivedEmptyDetail()) ?? "").toMatch(/archiv/i);
        const archived = await serverArchivedCycles(seed.workspaceSlug, emptyProjectId, session);
        expect(archived).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, emptyProjectId, session);
    }
  }
);

test(
  specTitle(["CYC-028"], "row menus offer state-valid entries in order, reachable on hover"),
  { tag: specTags(["CYC-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 menu ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const draft = `${tag} draft`;
    const done = `${tag} done`;
    const archivedName = `${tag} archived`;
    await createDraftCycle(seed, projectId, draft, session);
    await createCompletedCycle(seed, projectId, done, session);
    await createArchivedCycle(seed, projectId, archivedName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [draft], []);

      await test.step("a live unfinished row offers edit, link actions, gated archive, and delete", async () => {
        // One open per read: the trigger toggles, so opening twice in a
        // row closes the menu again. Titles and flags come from one read.
        await driver.archivesCyclesOpenRowMenu(draft);
        const entries = await driver.archivesCyclesMenuEntries();
        expect(entries.map((entry) => entry.title)).toEqual([
          "Edit",
          "Open in new tab",
          "Copy link",
          "Archive",
          "Delete",
        ]);
        const byTitle = new Map(entries.map((entry) => [entry.title, entry]));
        expect(byTitle.get("Archive")?.disabled).toBe(true);
        expect(byTitle.get("Archive")?.description ?? "").toMatch(/completed/i);
        expect(byTitle.get("Open in new tab")?.disabled).toBe(false);
        expect(byTitle.get("Copy link")?.disabled).toBe(false);
        expect(byTitle.get("Edit")?.disabled).toBe(false);
        expect(byTitle.get("Delete")?.disabled).toBe(false);
      });

      await test.step("a completed row drops edit and delete and enables archive", async () => {
        await openLiveSettled(driver, seed.workspaceSlug, projectId, [done]);
        await driver.archivesCyclesOpenRowMenu(done);
        const entries = await driver.archivesCyclesMenuEntries();
        expect(entries.map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link", "Archive"]);
        const byTitle = new Map(entries.map((entry) => [entry.title, entry]));
        expect(byTitle.get("Archive")?.disabled).toBe(false);
        expect(byTitle.get("Open in new tab")?.disabled).toBe(false);
        expect(byTitle.get("Copy link")?.disabled).toBe(false);
      });

      await test.step("an archived row swaps archive for restore", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [archivedName]);
        await driver.archivesCyclesOpenRowMenu(archivedName);
        const entries = await driver.archivesCyclesMenuEntries();
        expect(entries.map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link", "Restore"]);
        const byTitle = new Map(entries.map((entry) => [entry.title, entry]));
        expect(byTitle.get("Restore")?.disabled).toBe(false);
        expect(byTitle.get("Open in new tab")?.disabled).toBe(false);
        expect(byTitle.get("Copy link")?.disabled).toBe(false);
      });

      await test.step("the desktop row exposes its menu trigger on hover", async () => {
        // Observed: the trigger renders without hovering on desktop; the
        // hover is the desktop way to reach the menu (touch inline menus
        // belong to the hero/cross-cutting child). Its own phase on a
        // fresh list, after the menu reads above.
        await openLiveSettled(driver, seed.workspaceSlug, projectId, [draft], []);
        await driver.cyclesRowHover(draft);
        expect(await driver.cyclesRowMenuTriggerVisible(draft)).toBe(true);
      });

      await test.step("menus mutate nothing by themselves", async () => {
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live).toHaveLength(2);
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived).toHaveLength(1);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-028"], "guests see only the link actions in row menus"),
  { tag: specTags(["CYC-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 guestmenu ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const guest = requireGuest(seed);
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [name], []);
      expect(await menuTitles(driver, name)).toEqual(["Open in new tab", "Copy link"]);

      const guestSession = await signInSession(guest.email, guest.password);
      const res = await serverRequestStatus(
        "POST",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
        guestSession
      );
      expect(res.status).toBe(403);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

/**
 * Open a cycle detail page until the transfer prompt renders, reopening
 * twice: the oracle dev server stalls whole renders under shared-stack
 * load. A genuinely absent prompt still fails the final 60s poll.
 */
async function openCycleSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  cycleId: string
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    await driver.openCycleIssues(workspaceSlug, projectId, cycleId);
    const settled = await expect
      .poll(() => driver.cycleTransferButtonVisible(), { timeout: 30_000 })
      .toBe(true)
      .then(
        () => true,
        () => false
      );
    if (settled) return;
  }
  await expect.poll(() => driver.cycleTransferButtonVisible(), { timeout: 60_000 }).toBe(true);
}

test(
  specTitle(["CYC-029"], "row favorite toggle marks, toasts progress and outcome, and persists"),
  { tag: specTags(["CYC-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 fav ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [name], []);
      expect(await driver.cyclesRowFavoriteState(name)).toBe("unstarred");

      await test.step("adding toasts progress, then the outcome, and marks the row", async () => {
        // The row occasionally swallows a toggle click (the click returns
        // but no request goes out), so a toggle with no loading toast and
        // no server flip is retried once; the server guard keeps a landed
        // click from toggling back.
        await driver.cyclesDelayFavoriteWrites(3000);
        try {
          for (let attempt = 1; attempt <= 2; attempt++) {
            await driver.cyclesRowFavoriteToggle(name);
            const sawLoading = await expect
              .poll(
                async () => {
                  const toast = await driver.rulesLastToast();
                  return toast ? `${toast.title} ${toast.message}` : "";
                },
                { timeout: 15_000 }
              )
              .toMatch(/adding/i)
              .then(
                () => true,
                () => false
              );
            if (sawLoading) break;
            if (await serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session)) break;
          }
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 30_000 }
            )
            .toMatch(/added/i);
        } finally {
          await driver.cyclesClearFavoriteDelays();
        }
        await expect
          .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 30_000 })
          .toBe(true);
        await expect.poll(() => driver.cyclesRowFavoriteState(name), { timeout: 30_000 }).toBe("starred");
      });

      await test.step("removing toasts the outcome and unmarks the row", async () => {
        for (let attempt = 1; attempt <= 2; attempt++) {
          await driver.cyclesRowFavoriteToggle(name);
          const removed = await expect
            .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 15_000 })
            .toBe(false)
            .then(
              () => true,
              () => false
            );
          if (removed) break;
        }
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 30_000 }
          )
          .toMatch(/removed/i);
        await expect.poll(() => driver.cyclesRowFavoriteState(name), { timeout: 30_000 }).toBe("unstarred");
        await expect
          .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 30_000 })
          .toBe(false);
      });
    } finally {
      await driver.cyclesClearFavoriteDelays().catch(() => undefined);
      await cyclesUnfavoriteCycle(seed.workspaceSlug, projectId, cycleId, session).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-029"], "the first-ever favorite reveals the favorites menu"),
  { tag: specTags(["CYC-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 firstfav ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    try {
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await cyclesUnfavoriteCycle(seed.workspaceSlug, projectId, cycleId, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [name], []);

      await test.step("with no favorites the sidebar shows no favorites section", async () => {
        expect(await listFavorites(seed.workspaceSlug, session)).toEqual([]);
        expect(await driver.sidebarSectionNames()).not.toContain("Favorites");
      });

      await test.step("favoriting the cycle surfaces the section with the entry", async () => {
        for (let attempt = 1; attempt <= 2; attempt++) {
          await driver.cyclesRowFavoriteToggle(name);
          const added = await expect
            .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 15_000 })
            .toBe(true)
            .then(
              () => true,
              () => false
            );
          if (added) break;
        }
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 30_000 }
          )
          .toMatch(/added/i);
        await expect.poll(() => driver.sidebarSectionNames(), { timeout: 30_000 }).toContain("Favorites");
        await driver.setFavoritesOpen(true);
        await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(name);
        const server = await listFavorites(seed.workspaceSlug, session);
        expect(server.map((fav) => fav.entity_identifier)).toContain(cycleId);
      });
    } finally {
      await cyclesUnfavoriteCycle(seed.workspaceSlug, projectId, cycleId, session).catch(() => undefined);
      for (const fav of await listFavorites(seed.workspaceSlug, session).catch(() => [])) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id).catch(() => undefined);
      }
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-030"], "cycle pages offer favorite toggle and copy-address; archived cycles offer neither"),
  { tag: specTags(["CYC-030"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 palette ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const archivedName = `${tag} archived`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    await createArchivedCycle(seed, projectId, archivedName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the live cycle palette offers a favorite toggle plus copy-address", async () => {
        await driver.openCycleIssues(seed.workspaceSlug, projectId, cycleId);
        await expect.poll(() => driver.hasVisibleText(name), { timeout: 120_000 }).toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
        expect(await driver.paletteHasCommand("Add to favorites")).toBe(true);
        expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
      });

      await test.step("the palette toggle flips the mark in both directions", async () => {
        await driver.activatePaletteCommand("Add to favorites");
        await expect
          .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 30_000 })
          .toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
        expect(await driver.paletteHasCommand("Remove from favorites")).toBe(true);
        await driver.activatePaletteCommand("Remove from favorites");
        await expect
          .poll(() => serverCycleIsFavorite(seed.workspaceSlug, projectId, cycleId, session), { timeout: 30_000 })
          .toBe(false);
      });

      await test.step("the archived tab palette offers no cycle commands", async () => {
        await openTabViaLiveSettled(driver, seed.workspaceSlug, projectId, [archivedName]);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
        expect(await driver.paletteHasCommand("Add to favorites")).toBe(false);
        expect(await driver.paletteHasCommand("Remove from favorites")).toBe(false);
        expect(await driver.paletteHasCommand("Copy URL")).toBe(false);
      });
    } finally {
      await cyclesUnfavoriteCycle(seed.workspaceSlug, projectId, cycleId, session).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-031"], "transfer offers only open cycles, narrows by search, and refreshes both sides"),
  { tag: specTags(["CYC-031"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 transfer ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const issueA = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue a`, session);
    const issueB = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue b`, session);
    const sourceName = `${tag} source`;
    const targetA = `${tag} target a`;
    const targetB = `${tag} target b`;
    const otherDone = `${tag} other done`;
    const archivedName = `${tag} archived`;
    // The source starts current (attaching to a completed cycle is a 400),
    // takes the issues, then backdates into COMPLETED. Targets stay open so
    // the dialog lists them; the other finished cycle and the archived one
    // must never be offered.
    const sourceId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      sourceName,
      CURRENT_START,
      CURRENT_END,
      session
    );
    const targetAId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      targetA,
      CURRENT_START,
      CURRENT_END,
      session
    );
    const targetBId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      targetB,
      UPCOMING_START,
      UPCOMING_END,
      session
    );
    await createCompletedCycle(seed, projectId, otherDone, session);
    await createArchivedCycle(seed, projectId, archivedName, session);
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, sourceId, [issueA.id, issueB.id], session);
    await serverPatchCycle(
      seed.workspaceSlug,
      projectId,
      sourceId,
      { start_date: DONE_START, end_date: DONE_END },
      session
    );
    try {
      const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
      expect(cycles.find((row) => row.id === sourceId)?.status).toBe("COMPLETED");
      const before = await serverCycleProgress(seed.workspaceSlug, projectId, sourceId, session);
      expect(before.backlog + before.unstarted + before.started).toBeGreaterThan(0);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the dialog offers only the open cycles", async () => {
        await openCycleSettled(driver, seed.workspaceSlug, projectId, sourceId);
        await driver.cycleTransferOpen();
        await expect
          .poll(async () => (await driver.cycleTransferOptionNames()).some((text) => text.includes(targetA)), {
            timeout: 30_000,
          })
          .toBe(true);
        const options = await driver.cycleTransferOptionNames();
        expect(options.some((text) => text.includes(targetA))).toBe(true);
        expect(options.some((text) => text.includes(targetB))).toBe(true);
        expect(options.some((text) => text.includes(otherDone))).toBe(false);
        expect(options.some((text) => text.includes(archivedName))).toBe(false);
        expect(options.some((text) => text.includes(sourceName))).toBe(false);
        expect(options).toHaveLength(2);
        expect(targetAId).not.toBe(targetBId);
      });

      await test.step("search narrows the offered cycles", async () => {
        await driver.cyclesTransferSearchFill("target b");
        await expect
          .poll(async () => (await driver.cycleTransferOptionNames()).filter((text) => text.includes(tag)), {
            timeout: 30_000,
          })
          .toHaveLength(1);
        expect(await driver.cycleTransferOptionNames()).toEqual(
          expect.arrayContaining([expect.stringContaining(targetB)])
        );
        await driver.cyclesTransferSearchFill("");
        await expect.poll(() => driver.cycleTransferOptionNames(), { timeout: 30_000 }).toHaveLength(2);
      });

      await test.step("confirming moves the items and confirms", async () => {
        await driver.cycleTransferPick(targetB);
        expect(await toastText(driver)).toMatch(/transferred successfully/i);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issueA.id, session)).cycle_id, {
            timeout: 30_000,
          })
          .toBe(targetBId);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issueB.id, session)).cycle_id, {
            timeout: 30_000,
          })
          .toBe(targetBId);
      });

      await test.step("both sides refresh: the target lists the items, the source no longer does", async () => {
        await driver.openCycleIssues(seed.workspaceSlug, projectId, targetBId);
        await expect.poll(() => driver.globalViewIssueVisible(issueA.name), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.globalViewIssueVisible(issueB.name), { timeout: 60_000 }).toBe(true);
        await driver.openCycleIssues(seed.workspaceSlug, projectId, sourceId);
        await expect.poll(() => driver.globalViewIssueVisible(issueA.name), { timeout: 60_000 }).toBe(false);
        await expect.poll(() => driver.globalViewIssueVisible(issueB.name), { timeout: 60_000 }).toBe(false);
        expect((await serverCycleProgress(seed.workspaceSlug, projectId, targetBId, session)).total).toBe(2);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issueA.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issueB.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-031"], "with no open cycle the transfer dialog explains one must be created"),
  { tag: specTags(["CYC-031"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 notarget ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const sourceName = `${tag} source`;
    const sourceId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      sourceName,
      CURRENT_START,
      CURRENT_END,
      session
    );
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, sourceId, [issue.id], session);
    await serverPatchCycle(
      seed.workspaceSlug,
      projectId,
      sourceId,
      { start_date: DONE_START, end_date: DONE_END },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openCycleSettled(driver, seed.workspaceSlug, projectId, sourceId);
      await driver.cycleTransferOpen();
      await expect
        .poll(
          async () =>
            (await driver.cycleTransferOptionNames()).length === 0 && (await driver.cyclesTransferEmptyText()) !== null,
          { timeout: 30_000 }
        )
        .toBe(true);
      expect((await driver.cyclesTransferEmptyText()) ?? "").toMatch(/creat/i);
      expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).cycle_id).toBe(sourceId);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-046"], "the row menu copies the cycle address with confirmation and opens it in a new tab"),
  { tag: specTags(["CYC-046"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 rowlink ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    const address = `/projects/${projectId}/cycles/${cycleId}`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openLiveSettled(driver, seed.workspaceSlug, projectId, [name], []);

      await test.step("copying the link confirms and clips the cycle address", async () => {
        await driver.cyclesGrantClipboardAccess();
        await driver.archivesCyclesOpenRowMenu(name);
        await driver.archivesCyclesMenuPick("Copy link");
        expect(await toastText(driver)).toMatch(/cop/i);
        expect(await driver.readClipboard()).toContain(address);
      });

      await test.step("opening in a new tab lands on the same cycle", async () => {
        const url = await driver.cyclesRowMenuOpenNewTab(name);
        expect(url).toContain(address);
      });

      await test.step("link actions mutate nothing", async () => {
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.id)).toContain(cycleId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-046"], "the palette copies the same cycle address with confirmation"),
  { tag: specTags(["CYC-046"]) },
  async ({ driver, seed }) => {
    const tag = `NF252 palettelink ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N252"),
      { cycleView: true },
      session
    );
    const name = `${tag} cycle`;
    const cycleId = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
    const address = `/projects/${projectId}/cycles/${cycleId}`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cyclesGrantClipboardAccess();
      await driver.openCycleIssues(seed.workspaceSlug, projectId, cycleId);
      await expect.poll(() => driver.hasVisibleText(name), { timeout: 120_000 }).toBe(true);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen(), { timeout: 30_000 }).toBe(true);
      await driver.activatePaletteCommand("Copy URL");
      expect(await toastText(driver)).toMatch(/cop/i);
      expect(await driver.readClipboard()).toContain(address);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
