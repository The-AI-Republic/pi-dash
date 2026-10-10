// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-250): live cycles list empty states — the
// cause-specific no-match view, the feature-off view with its admin-gated
// shortcut, the first-run zero-state with its member-only shortcut, the
// loading skeleton transition, the detail gone-away view, cycle creation
// with optional description/dates plus cross-project targeting,
// client-side creation validation, and date-overlap checking with the
// undated-draft workaround.
// Rows: CYC-009–CYC-016.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateProjectWithFlags,
  serverCycleDateCheck,
  serverCycleDetail,
  serverDeleteCycle,
  serverEnsureProjectGuest,
  serverProjectCycles,
  serverProjectCycleView,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** ISO calendar date (local time) `days` from today: future fixtures that never rot. */
function isoDaysFromNow(days: number): string {
  const target = new Date();
  target.setDate(target.getDate() + days);
  const month = String(target.getMonth() + 1).padStart(2, "0");
  const day = String(target.getDate()).padStart(2, "0");
  return `${target.getFullYear()}-${month}-${day}`;
}

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
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
      { timeout: 15_000 }
    )
    .not.toBe("");
  return text;
}

/**
 * Open the live list until `wanted` names render, retrying the whole
 * navigation once: the oracle dev server stalls whole renders under
 * shared-stack load, and a stalled first pass must not fail an
 * assertion. Genuinely absent rows still fail.
 */
async function openListSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  wanted: string[]
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    try {
      await driver.cyclesEmptyOpenList(workspaceSlug, projectId);
      await expect
        .poll(
          async () => {
            const names = await driver.cyclesEmptyVisibleNames();
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

test(
  specTitle(["CYC-009"], "filters-hide-all and search-hides-all show distinct no-match hints"),
  { tag: specTags(["CYC-009"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 nomatch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const name = `${tag} upcoming`;
    await serverCreateCycle(seed.workspaceSlug, projectId, name, isoDaysFromNow(30), isoDaysFromNow(45), session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [name]);

      await test.step("an excluding status filter shows the filters no-match hint", async () => {
        await driver.cyclesEmptyFiltersOpen();
        const sections = await driver.cyclesEmptyFilterSections();
        expect(sections).toContain("Status of the cycle");
        expect(await driver.cyclesEmptyFilterOptionNames()).toContain("Completed");
        await driver.cyclesEmptyFilterPick("Status of the cycle", "Completed");
        await driver.cyclesEmptyFiltersClose();
        await expect.poll(() => driver.cyclesEmptyNoMatchHint(), { timeout: 30_000 }).not.toBe(null);
        expect(await driver.cyclesEmptyNoMatchHeading()).not.toBe(null);
        expect(await driver.cyclesEmptyVisibleNames()).toEqual([]);
        const filtersHint = await driver.cyclesEmptyNoMatchHint();
        expect(filtersHint as string).toMatch(/filter/i);
        await driver.cyclesEmptyFiltersClearAll();
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toEqual([name]);
      });

      await test.step("a matching-nothing search shows the search no-match hint", async () => {
        await driver.cyclesEmptySearchOpen();
        await driver.cyclesEmptySearchFill("zzz-no-such-orbit");
        await expect.poll(() => driver.cyclesEmptyNoMatchHint(), { timeout: 30_000 }).not.toBe(null);
        const searchHint = await driver.cyclesEmptyNoMatchHint();
        expect(searchHint as string).toMatch(/search/i);
        expect(await driver.cyclesEmptyVisibleNames()).toEqual([]);
        await driver.cyclesEmptySearchClear();
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toEqual([name]);
      });

      await test.step("the two hints differ and server state is untouched", async () => {
        await driver.cyclesEmptyFiltersOpen();
        await driver.cyclesEmptyFilterPick("Status of the cycle", "Completed");
        await driver.cyclesEmptyFiltersClose();
        const filtersHint = await expect
          .poll(() => driver.cyclesEmptyNoMatchHint(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.cyclesEmptyNoMatchHint());
        await driver.cyclesEmptyFiltersClearAll();
        await driver.cyclesEmptySearchOpen();
        await driver.cyclesEmptySearchFill("zzz-no-such-orbit");
        const searchHint = await expect
          .poll(() => driver.cyclesEmptyNoMatchHint(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.cyclesEmptyNoMatchHint());
        expect(filtersHint).not.toBe(searchHint);
        await driver.cyclesEmptySearchClear();
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles.map((row) => row.name)).toEqual([name]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-010"], "feature-off project explains itself and routes admins to feature settings"),
  { tag: specTags(["CYC-010"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 featureoff ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: false },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the feature-off view renders with an enabled shortcut", async () => {
        await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);
        expect(await driver.cyclesEmptyFeatureOffVisible()).toBe(true);
        expect(await driver.cyclesEmptyFeatureOffActionDisabled()).toBe(false);
        expect(await driver.cyclesEmptyVisibleNames()).toEqual([]);
      });

      await test.step("the shortcut routes admins to project feature settings", async () => {
        await driver.cyclesEmptyFeatureOffActionOpen();
        await expect
          .poll(() => driver.currentPath(), { timeout: 30_000 })
          .toBe(`/${seed.workspaceSlug}/settings/projects/${projectId}/features/`);
      });

      await test.step("the server agrees cycles are off", async () => {
        expect(await serverProjectCycleView(seed.workspaceSlug, projectId, session)).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-010"], "feature-off shortcut is disabled for guests"),
  { tag: specTags(["CYC-010"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 featureoff guest ${Date.now()}`;
    const guest = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: false },
      session
    );
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);

      await test.step("guests see the explanation but no working shortcut", async () => {
        await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);
        expect(await driver.cyclesEmptyFeatureOffVisible()).toBe(true);
        expect(await driver.cyclesEmptyFeatureOffActionDisabled()).toBe(true);
      });

      await test.step("the server agrees cycles are off", async () => {
        expect(await serverProjectCycleView(seed.workspaceSlug, projectId, session)).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-011"], "first-run zero-state offers creation to members"),
  { tag: specTags(["CYC-011"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 zerostate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a project with no cycles renders the zero-state shortcut", async () => {
        await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);
        expect(await driver.cyclesEmptyZeroStateVisible()).toBe(true);
        expect(await driver.cyclesEmptyZeroStateCreateDisabled()).toBe(false);
        expect(await driver.cyclesEmptyVisibleNames()).toEqual([]);
      });

      await test.step("the shortcut opens the create dialog", async () => {
        await driver.cyclesEmptyZeroStateCreateOpen();
        expect(await driver.cyclesCreateDialogOpen()).toBe(true);
        await driver.cyclesCreateDialogCancel();
        expect(await driver.cyclesCreateDialogOpen()).toBe(false);
      });

      await test.step("the server agrees the project has no cycles", async () => {
        expect(await serverProjectCycles(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-011"], "guests see the zero-state but cannot trigger creation from it"),
  { tag: specTags(["CYC-011"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 zerostate guest ${Date.now()}`;
    const guest = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);

      await test.step("guests see the view with the shortcut disabled", async () => {
        await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);
        expect(await driver.cyclesEmptyZeroStateVisible()).toBe(true);
        expect(await driver.cyclesEmptyZeroStateCreateDisabled()).toBe(true);
        expect(await driver.cyclesCreateDialogOpen()).toBe(false);
      });

      await test.step("the server agrees the project has no cycles", async () => {
        expect(await serverProjectCycles(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-012"], "list skeleton shows mid-fetch and yields to rows or an empty view"),
  { tag: specTags(["CYC-012"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 skeleton ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const filledId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} filled project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const emptyId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} empty project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const name = `${tag} upcoming`;
    await serverCreateCycle(seed.workspaceSlug, filledId, name, isoDaysFromNow(30), isoDaysFromNow(45), session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a filled project shows the skeleton, then its rows", async () => {
        expect(await driver.cyclesEmptySkeletonShownOnSlowFetch(seed.workspaceSlug, filledId)).toBe(true);
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toEqual([name]);
        expect(await driver.cyclesEmptySkeletonVisible()).toBe(false);
      });

      await test.step("a zero-cycle project shows the skeleton, then the zero-state", async () => {
        expect(await driver.cyclesEmptySkeletonShownOnSlowFetch(seed.workspaceSlug, emptyId)).toBe(true);
        expect(await driver.cyclesEmptyZeroStateVisible()).toBe(true);
        expect(await driver.cyclesEmptySkeletonVisible()).toBe(false);
      });

      await test.step("the server agrees on filled vs empty membership", async () => {
        expect((await serverProjectCycles(seed.workspaceSlug, filledId, session)).map((row) => row.name)).toEqual([
          name,
        ]);
        expect(await serverProjectCycles(seed.workspaceSlug, emptyId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, filledId, session);
      await serverCleanupProject(seed.workspaceSlug, emptyId, session);
    }
  }
);

test(
  specTitle(["CYC-013"], "missing and deleted cycles render the gone-away view with a way back"),
  { tag: specTags(["CYC-013"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 goneaway ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const doomedId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      `${tag} doomed`,
      isoDaysFromNow(30),
      isoDaysFromNow(45),
      session
    );
    await serverDeleteCycle(seed.workspaceSlug, projectId, doomedId, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a never-existing id renders the gone-away view", async () => {
        await driver.cyclesEmptyOpenDetail(seed.workspaceSlug, projectId, "00000000-0000-0000-0000-000000000000");
        await expect.poll(() => driver.cyclesEmptyGoneAwayVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("back-to-list returns to the cycles list", async () => {
        await driver.cyclesEmptyGoneAwayBack();
        expect(await driver.currentPath()).toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles/`);
      });

      await test.step("a deleted id renders the same gone-away view", async () => {
        await driver.cyclesEmptyOpenDetail(seed.workspaceSlug, projectId, doomedId);
        await expect.poll(() => driver.cyclesEmptyGoneAwayVisible(), { timeout: 30_000 }).toBe(true);
        await driver.cyclesEmptyGoneAwayBack();
        expect(await driver.currentPath()).toBe(`/${seed.workspaceSlug}/projects/${projectId}/cycles/`);
      });

      await test.step("the server agrees the project has no cycles", async () => {
        expect(await serverProjectCycles(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-014"], "header creation saves title, description, and dates, then lists the row"),
  { tag: specTags(["CYC-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 create ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const name = `${tag} planned`;
    const description = `${tag} description`;
    const start = isoDaysFromNow(30);
    const end = isoDaysFromNow(45);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);
      expect(await driver.cyclesEmptyZeroStateVisible()).toBe(true);

      await test.step("valid input creates the cycle and confirms", async () => {
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateFillTitle(name);
        await driver.cyclesCreateFillDescription(description);
        await driver.cyclesCreatePickDateRange(start, end);
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/creat/i);
      });

      await test.step("the dialog closes and the new row appears", async () => {
        await expect.poll(() => driver.cyclesCreateDialogOpen(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toContain(name);
      });

      await test.step("the server stored title, description, and dates", async () => {
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles.map((row) => row.name)).toEqual([name]);
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, cycles[0]?.id as string, session);
        expect(detail.name).toBe(name);
        expect(detail.description).toBe(description);
        // The server stores datetimes (with a 1s normalization offset);
        // the picked calendar days are the creation contract.
        expect(detail.startDate?.slice(0, 10)).toBe(start);
        expect(detail.endDate?.slice(0, 10)).toBe(end);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-014"], "creation dialog targets another project via its project picker"),
  { tag: specTags(["CYC-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 create cross ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectAName = `${tag} project a`;
    const projectBName = `${tag} project b`;
    const projectAId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectAName,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const projectBId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectBName,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const name = `${tag} moved`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectAId);

      await test.step("picking another project retargets the creation", async () => {
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreatePickProject(projectBName);
        await driver.cyclesCreateFillTitle(name);
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/creat/i);
        await expect.poll(() => driver.cyclesCreateDialogOpen(), { timeout: 30_000 }).toBe(false);
      });

      await test.step("the cycle lands in the picked project only", async () => {
        expect(await serverProjectCycles(seed.workspaceSlug, projectAId, session)).toEqual([]);
        const cyclesB = await serverProjectCycles(seed.workspaceSlug, projectBId, session);
        expect(cyclesB.map((row) => row.name)).toEqual([name]);
      });

      await test.step("the picked project's list shows the new row", async () => {
        await openListSettled(driver, seed.workspaceSlug, projectBId, [name]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectAId, session);
      await serverCleanupProject(seed.workspaceSlug, projectBId, session);
    }
  }
);

test(
  specTitle(["CYC-015"], "empty and overlong titles are refused inline without saving"),
  { tag: specTags(["CYC-015"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 validation ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);

      await test.step("an empty title blocks submit with an inline error", async () => {
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateSubmit();
        const error = await expect
          .poll(() => driver.cyclesCreateTitleError(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.cyclesCreateTitleError());
        expect(error as string).toMatch(/title|required/i);
        expect(await driver.cyclesCreateDialogOpen()).toBe(true);
        expect(await serverProjectCycles(seed.workspaceSlug, projectId, session)).toEqual([]);
      });

      await test.step("filling the title recovers and saves", async () => {
        const name = `${tag} recovered`;
        await driver.cyclesCreateFillTitle(name);
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/creat/i);
        await expect.poll(() => driver.cyclesCreateDialogOpen(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toContain(name);
      });

      await test.step("an overlong title is refused without saving", async () => {
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateFillTitle("x".repeat(256));
        await driver.cyclesCreateSubmit();
        const error = await expect
          .poll(() => driver.cyclesCreateTitleError(), { timeout: 30_000 })
          .not.toBe(null)
          .then(() => driver.cyclesCreateTitleError());
        expect(error as string).toMatch(/title|255/i);
        expect(await driver.cyclesCreateDialogOpen()).toBe(true);
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles).toHaveLength(1);
        await driver.cyclesCreateDialogCancel();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-015"], "past start dates cannot be picked in the dialog"),
  { tag: specTags(["CYC-015"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 validation past ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.cyclesEmptyOpenList(seed.workspaceSlug, projectId);

      await test.step("yesterday's day cell is disabled in the start picker", async () => {
        await driver.cyclesCreateOpenFromHeader();
        expect(await driver.cyclesCreatePastDayDisabled()).toBe(true);
        await driver.cyclesCreateDialogCancel();
      });

      await test.step("nothing was saved", async () => {
        expect(await serverProjectCycles(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["CYC-016"], "overlapping ranges are refused naming the draft workaround; free and undated save"),
  { tag: specTags(["CYC-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF250 overlap ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N250"),
      { cycleView: true },
      session
    );
    const existing = `${tag} existing`;
    await serverCreateCycle(seed.workspaceSlug, projectId, existing, isoDaysFromNow(10), isoDaysFromNow(20), session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openListSettled(driver, seed.workspaceSlug, projectId, [existing]);

      await test.step("an overlapping range is refused naming the draft workaround", async () => {
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateFillTitle(`${tag} overlapping`);
        await driver.cyclesCreatePickDateRange(isoDaysFromNow(15), isoDaysFromNow(25));
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/draft/i);
        expect(await driver.cyclesCreateDialogOpen()).toBe(true);
        expect((await serverProjectCycles(seed.workspaceSlug, projectId, session)).map((row) => row.name)).toEqual([
          existing,
        ]);
        await driver.cyclesCreateDialogCancel();
      });

      await test.step("a non-overlapping range saves", async () => {
        const name = `${tag} separate`;
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateFillTitle(name);
        await driver.cyclesCreatePickDateRange(isoDaysFromNow(30), isoDaysFromNow(40));
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/creat/i);
        await expect.poll(() => driver.cyclesCreateDialogOpen(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toContain(name);
      });

      await test.step("an undated draft cycle saves", async () => {
        const name = `${tag} draft`;
        await driver.cyclesCreateOpenFromHeader();
        await driver.cyclesCreateFillTitle(name);
        await driver.cyclesCreateSubmit();
        expect(await toastText(driver)).toMatch(/creat/i);
        await expect.poll(() => driver.cyclesEmptyVisibleNames(), { timeout: 30_000 }).toContain(name);
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        const draft = cycles.find((row) => row.name === name);
        expect(draft).toBeDefined();
        const detail = await serverCycleDetail(seed.workspaceSlug, projectId, draft?.id as string, session);
        expect(detail.startDate).toBe(null);
        expect(detail.endDate).toBe(null);
      });

      await test.step("the date-check endpoint agrees with the dialog verdicts", async () => {
        expect(
          await serverCycleDateCheck(
            seed.workspaceSlug,
            projectId,
            { start_date: isoDaysFromNow(15), end_date: isoDaysFromNow(25) },
            session
          )
        ).toBe(false);
        expect(
          await serverCycleDateCheck(
            seed.workspaceSlug,
            projectId,
            { start_date: isoDaysFromNow(50), end_date: isoDaysFromNow(60) },
            session
          )
        ).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
