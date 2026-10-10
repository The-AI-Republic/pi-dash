// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-251): cycles edit, dialog, delete,
// finished read-only, archive — editing through the reused creation form
// with overlap-check skipping, post-creation landing on the remembered
// "all" tab, active-cycle refresh when a created cycle covers today,
// dialog Escape/keyboard behavior, creation gating for guests, delete
// confirmation with return-to-list, finished-cycle read-only gating, and
// archiving a finished cycle through confirmation.
//
// The suite reuses settled cross-area helpers (cycle create-form and
// range-calendar methods, the archived-cycles row-menu and archive-dialog
// readers, toast and path reads) and adds `cyclesEdit*` driver methods
// only for genuinely new surface. Adjacent spec
// archives/cycles.spec.ts (ARCH-014–019) stays read-only.
// Rows: CYC-017–CYC-024.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverArchivedCycles,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateProjectWithFlags,
  serverCycleDetail,
  serverEnsureProjectGuest,
  serverPatchCycle,
  serverProjectCycles,
  serverRequestStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

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

/** ISO date `days` from today (all cycle ranges derive from this, so the suite never rots). */
function isoIn(days: number): string {
  const at = new Date();
  at.setDate(at.getDate() + days);
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
}

function partsOf(iso: string): { year: number; month0: number; day: number } {
  const [year, month, day] = iso.split("-").map(Number);
  return { year: year ?? 0, month0: (month ?? 1) - 1, day: day ?? 1 };
}

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

async function scratchProject(seed: ParitySeedFacts, tag: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N251"),
    { cycleView: true },
    session
  );
}

/**
 * Create a cycle reading finished: the create call takes current-covering
 * dates (the server rejects finished-looking creates), then a patch
 * backdates it. Resolves with the cycle id.
 */
async function createFinishedCycle(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  session: string
): Promise<string> {
  const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, isoIn(-7), isoIn(60), session);
  await serverPatchCycle(seed.workspaceSlug, projectId, id, { start_date: isoIn(-100), end_date: isoIn(-70) }, session);
  return id;
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

/**
 * Pick a from/to pair in the open range calendar, navigating months and
 * years explicitly so month boundaries never matter, then close it.
 */
async function pickRange(driver: ParityDriver, fromIso: string, toIso: string): Promise<void> {
  const from = partsOf(fromIso);
  const to = partsOf(toIso);
  await driver.rangeCalendarSelectYear(String(from.year));
  await driver.rangeCalendarSelectMonth(MONTH_FULL[from.month0] as string);
  await driver.rangeCalendarPickDay(from.day);
  await driver.rangeCalendarSelectYear(String(to.year));
  await driver.rangeCalendarSelectMonth(MONTH_FULL[to.month0] as string);
  await driver.rangeCalendarPickDay(to.day);
  await driver.pickerPressEscape();
  await expect.poll(() => driver.rangeCalendarVisible(), { timeout: 10_000 }).toBe(false);
}

test(
  specTitle(["CYC-017"], "editing without touching dates saves without an overlap check"),
  { tag: specTags(["CYC-017"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 noskip ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    const renamed = `${tag} renamed`;
    const start = isoIn(70);
    const end = isoIn(100);
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, name, start, end, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);

      await test.step("rename-only edit fires no date-check", async () => {
        await driver.cyclesEditOpenUpdateDialog(name);
        await driver.cyclesEditFillName(renamed);
        const { dateChecks } = await driver.cyclesEditSubmitCountingDateChecks();
        expect(dateChecks).toBe(0);
      });

      await test.step("rename saved on the server with dates untouched", async () => {
        expect(await toastText(driver)).toMatch(/updated successfully/i);
        expect(await driver.archivesCyclesVisibleNames()).toContain(renamed);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name)).toContain(renamed);
        expect(rows.map((row) => row.name)).not.toContain(name);
        const id = rows.find((row) => row.name === renamed)?.id ?? "";
        const updated = await serverCycleDetail(seed.workspaceSlug, projectId, id, session);
        expect(updated.startDate?.slice(0, 10)).toBe(start);
        expect(updated.endDate?.slice(0, 10)).toBe(end);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-017"], "editing with new dates re-checks overlaps before saving"),
  { tag: specTags(["CYC-017"]) },
  async ({ driver, seed }) => {
    // Only the end date moves: with a range already set, picking a day on
    // or after the current start replaces the end (probed), so one pick
    // past the current end is the deterministic change.
    const tag = `NF251 recheck ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    const start = isoIn(70);
    const end = isoIn(140);
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, name, start, isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);

      await test.step("changed dates fire the overlap check and save", async () => {
        await driver.cyclesEditOpenUpdateDialog(name);
        await driver.cyclesEditRangeOpen();
        const to = partsOf(end);
        await driver.rangeCalendarSelectYear(String(to.year));
        await driver.rangeCalendarSelectMonth(MONTH_FULL[to.month0] as string);
        await driver.rangeCalendarPickDay(to.day);
        await driver.pickerPressEscape();
        await expect.poll(() => driver.rangeCalendarVisible(), { timeout: 10_000 }).toBe(false);
        const { dateChecks } = await driver.cyclesEditSubmitCountingDateChecks();
        expect(dateChecks).toBeGreaterThanOrEqual(1);
      });

      await test.step("new range persisted on the server", async () => {
        expect(await toastText(driver)).toMatch(/updated successfully/i);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        const id = rows.find((row) => row.name === name)?.id ?? "";
        const updated = await serverCycleDetail(seed.workspaceSlug, projectId, id, session);
        expect(updated.startDate?.slice(0, 10)).toBe(start);
        expect(updated.endDate?.slice(0, 10)).toBe(end);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-018"], "creating a cycle stores the all-cycles list tab"),
  { tag: specTags(["CYC-018"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 alltab ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} anchor`, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.cyclesEditClearStoredCycleTab();
      expect(await driver.cyclesEditStoredCycleTab()).toBeNull();

      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);
      await driver.cycleFormFillName(name);
      await driver.cycleFormSubmit();

      await test.step("stored tab reads all and the row shows", async () => {
        expect(await driver.cyclesEditStoredCycleTab()).toBe("all");
        expect(await driver.archivesCyclesVisibleNames()).toContain(name);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name)).toContain(name);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-019"], "creating a cycle covering today refreshes the active-cycle hero without a reload"),
  { tag: specTags(["CYC-019"]) },
  async ({ driver, seed }) => {
    // Refresh wiring only: the hero cards themselves belong to the
    // hero/cross-cutting child (NEWFRONT-254). This pins that the hero
    // flips from its empty view to the new cycle with no navigation.
    const tag = `NF251 heronew ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const anchor = `${tag} anchor`;
    const name = `${tag} current`;
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, anchor, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      expect(await driver.cyclesEditHeroCycleName()).toBeNull();

      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);
      await driver.cycleFormFillName(name);
      await driver.cycleFormRangeOpen();
      await pickRange(driver, isoIn(0), isoIn(30));
      await driver.cycleFormSubmit();

      await test.step("hero names the new cycle with no reload", async () => {
        await expect.poll(() => driver.cyclesEditHeroCycleName(), { timeout: 30_000 }).toBe(name);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        const id = rows.find((row) => row.name === name)?.id ?? "";
        expect(id).not.toBe("");
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, id, session);
        expect(detail.startDate?.slice(0, 10)).toBe(isoIn(0));
        expect(detail.endDate?.slice(0, 10)).toBe(isoIn(30));
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-020"], "Escape dismisses the dialog without saving"),
  { tag: specTags(["CYC-020"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 escapeno ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} anchor`, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);
      await driver.cycleFormFillName(name);
      await driver.cyclesEditPressEscape();

      await test.step("dialog closed and nothing created", async () => {
        await expect.poll(() => driver.cyclesEditDialogHeading(), { timeout: 15_000 }).toBeNull();
        expect(await driver.archivesCyclesVisibleNames()).not.toContain(name);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name)).not.toContain(name);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-020"], "the dialog focuses the title first and tabs through every control"),
  { tag: specTags(["CYC-020"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 taborder ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} anchor`, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);

      await test.step("title holds initial focus", async () => {
        await expect.poll(() => driver.cyclesEditTitleFocused(), { timeout: 15_000 }).toBe(true);
      });

      await test.step("tabbing visits every control in order", async () => {
        // Forward from the autofocused title: the positively-indexed
        // controls in order.
        const trail = await driver.cyclesEditFocusTrail(4);
        expect(trail).toEqual(["Title", "Description", "Start date End date", "Cancel", "Create cycle"]);
      });
      await driver.cyclesEditPressEscape();
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-020"], "bug: NEWFRONT-263 forward tabbing never reaches the project picker"),
  { tag: specTags(["CYC-020"]) },
  async ({ driver, seed }) => {
    // bug: NEWFRONT-263 — past submit, focus leaves the dialog into the
    // row menus' invisible buttons and wraps back to the title without
    // ever stopping on the project picker. Intended: the picker belongs
    // in the order and focus stays trapped in the dialog.
    const tag = `NF251 tabbug ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    try {
      await serverCreateCycle(seed.workspaceSlug, projectId, `${tag} anchor`, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cycleCreateOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.cyclesEditTitleFocused(), { timeout: 15_000 }).toBe(true);

      const trail = await driver.cyclesEditFocusTrail(40);
      expect(trail.join("|")).not.toContain(`${tag} project`);
      await driver.cyclesEditPressEscape();
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-021"], "guests see no working creation affordance"),
  { tag: specTags(["CYC-021"]) },
  async ({ driver, seed }) => {
    // The backend offers no viewer role (admin/member/guest only), so the
    // guest session probes the read-only class the row's "guests and
    // viewers" names.
    const tag = `NF251 guestno ${Date.now()}`;
    const guest = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const guestSession = await signInSession(guest.email, guest.password);
    const listed = await scratchProject(seed, `${tag} listed`, session);
    const empty = await scratchProject(seed, `${tag} empty`, session);
    try {
      await serverCreateCycle(seed.workspaceSlug, listed, `${tag} anchor`, isoIn(70), isoIn(100), session);
      await serverEnsureProjectGuest(seed.workspaceSlug, listed, guest.email, session);
      await serverEnsureProjectGuest(seed.workspaceSlug, empty, guest.email, session);
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);

      await test.step("header button absent on the list", async () => {
        await driver.archivesCyclesOpenLive(seed.workspaceSlug, listed);
        expect(await driver.cyclesEditCreateButtonVisible()).toBe(false);
      });

      await test.step("empty-state shortcut disabled on an empty project", async () => {
        await driver.cyclesEditOpenListRaw(seed.workspaceSlug, empty);
        await expect
          .poll(() => driver.cyclesEditEmptyCreateState(), { timeout: 30_000 })
          .toEqual({ visible: true, disabled: true });
        expect(await driver.cyclesEditDialogHeading()).toBeNull();
      });

      await test.step("the server refuses guest creation", async () => {
        const attempt = await serverRequestStatus(
          "POST",
          `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${listed}/cycles/`,
          guestSession,
          { name: `${tag} smuggled` }
        );
        expect(attempt.status).toBe(403);
        const rows = await serverProjectCycles(seed.workspaceSlug, listed, session);
        expect(rows.map((row) => row.name)).not.toContain(`${tag} smuggled`);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, listed, session);
      await serverCleanupProject(seed.workspaceSlug, empty, session);
    }
  }
);

test(
  specTitle(["CYC-022"], "deleting from the list removes the cycle and confirms"),
  { tag: specTags(["CYC-022"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 delrow ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);

      await test.step("confirm deletes and toasts success", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        await driver.archivesCyclesMenuPick("Delete");
        const dialog = await driver.cyclesEditDeleteDialogText();
        expect(dialog?.heading).toMatch(/delete cycle/i);
        expect(dialog?.body ?? "").toContain(name);
        await driver.cyclesEditDeleteConfirm();
        expect(await toastText(driver)).toMatch(/deleted successfully/i);
      });

      await test.step("row gone from the UI and the server", async () => {
        expect(await driver.archivesCyclesVisibleNames()).not.toContain(name);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.id)).not.toContain(id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-022"], "deleting from detail returns to the list"),
  { tag: specTags(["CYC-022"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 deldet ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openCyclePage(seed.workspaceSlug, projectId, id);

      await driver.cyclesEditOpenDetailMenu();
      await driver.archivesCyclesMenuPick("Delete");
      const dialog = await driver.cyclesEditDeleteDialogText();
      expect(dialog?.heading).toMatch(/delete cycle/i);
      await driver.cyclesEditDeleteConfirm();

      await test.step("back on the list with the cycle gone", async () => {
        expect(await toastText(driver)).toMatch(/deleted successfully/i);
        // Poll: the list navigation commits asynchronously after the
        // write, and a single read can land before it does.
        await expect
          .poll(() => currentPathClean(driver), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.id)).not.toContain(id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-022"], "deleting with quick-look open returns to the plain list"),
  { tag: specTags(["CYC-022"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 delpeek ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, isoIn(70), isoIn(100), session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.cyclesEditOpenPeek(name);
      expect(await driver.archivesCyclesPeekParam()).toBe(id);

      await driver.archivesCyclesOpenRowMenu(name);
      await driver.archivesCyclesMenuPick("Delete");
      await driver.cyclesEditDeleteConfirm();

      await test.step("peek cleared and cycle gone", async () => {
        expect(await toastText(driver)).toMatch(/deleted successfully/i);
        // Poll: the return navigation commits asynchronously after the
        // write, and single reads can land before it does.
        await expect.poll(() => driver.archivesCyclesPeekParam(), { timeout: 30_000 }).toBeNull();
        await expect
          .poll(() => currentPathClean(driver), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles`);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.id)).not.toContain(id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-022"], "a forbidden delete maps to the permission message"),
  { tag: specTags(["CYC-022"]) },
  async ({ driver, seed }) => {
    // Guests cannot reach the confirm dialog at all (no menu entry), so
    // the UI mapping is pinned by failing the write once with the exact
    // refusal a low-privilege session receives; a genuine guest DELETE
    // proves the server side of the same mapping.
    const tag = `NF251 delperm ${Date.now()}`;
    const guest = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const guestSession = await signInSession(guest.email, guest.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, isoIn(70), isoIn(100), session);
      await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);

      await driver.archivesCyclesOpenRowMenu(name);
      await driver.archivesCyclesMenuPick("Delete");
      await driver.cyclesEditFailNextDeleteWrite();
      await driver.cyclesEditDeleteConfirm();

      await test.step("dedicated permission toast and the cycle survives", async () => {
        const toast = await toastText(driver);
        expect(toast).toMatch(/permission/i);
        expect(toast).not.toMatch(/failed to delete/i);
        expect(await driver.archivesCyclesVisibleNames()).toContain(name);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.id)).toContain(id);
      });

      await test.step("the server refuses a genuine guest delete", async () => {
        const attempt = await serverRequestStatus(
          "DELETE",
          `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/cycles/${id}/`,
          guestSession
        );
        expect(attempt.status).toBe(403);
        expect(attempt.bodyText).toMatch(/required permissions/i);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-023"], "finished cycles hide edit and delete and explain read-only on detail"),
  { tag: specTags(["CYC-023"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 readonly ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await createFinishedCycle(seed, projectId, name, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");

      await test.step("row menu carries no edit or delete entry", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        const titles = (await driver.archivesCyclesMenuEntries()).map((entry) => entry.title);
        expect(titles).not.toContain("Edit");
        expect(titles).not.toContain("Delete");
        await driver.cyclesEditPressEscape();
      });

      await test.step("Detail explains the cycle is read-only", async () => {
        // Plain entry: finished cycles hide work-item creation, so the
        // settled detail opener (which waits for the Add action) never
        // resolves here; the notice itself proves the detail rendered.
        await driver.openPath(`/${seed.workspaceSlug}/projects/${projectId}/cycles/${id}`);
        await expect.poll(() => driver.cyclesEditDetailReadOnlyNotice(), { timeout: 60_000 }).not.toBeNull();
        const notice = await driver.cyclesEditDetailReadOnlyNotice();
        expect(notice ?? "").toMatch(/not editable/i);
        const rows = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(rows.find((row) => row.id === id)?.status.toLowerCase()).toBe("completed");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-024"], "archiving a finished cycle confirms and leaves the active list"),
  { tag: specTags(["CYC-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF251 archok ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await scratchProject(seed, tag, session);
    const name = `${tag} cycle`;
    try {
      const id = await createFinishedCycle(seed, projectId, name, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesCyclesOpenLive(seed.workspaceSlug, projectId);
      await driver.archivesCyclesLiveGroupOpen("Completed");

      await test.step("archive confirms through its dialog", async () => {
        await driver.archivesCyclesOpenRowMenu(name);
        await driver.archivesCyclesMenuPick("Archive");
        const dialog = await driver.archivesCyclesArchiveDialogText();
        expect(dialog?.heading ?? "").toContain(name);
        await driver.archivesCyclesArchiveDialogConfirm();
      });

      await test.step("success points at project archives and the row moves", async () => {
        const toast = await toastText(driver);
        expect(toast).toMatch(/archive success/i);
        expect(toast).toMatch(/project archives/i);
        expect(await driver.archivesCyclesVisibleNames()).not.toContain(name);
        const live = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(live.map((row) => row.id)).not.toContain(id);
        const archived = await serverArchivedCycles(seed.workspaceSlug, projectId, session);
        expect(archived.map((row) => row.id)).toContain(id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
