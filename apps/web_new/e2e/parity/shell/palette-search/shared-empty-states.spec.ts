// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared empty-state kit (NEWFRONT-127). Row: SHELL-104. The kit's tiers
// are distinguished by rendered structure: Detailed leads with text plus
// themed art and action buttons (the cycles list, whose feature is off on
// the seeded project, and the fresh cycle detail); Simple centers art with
// a heading and never renders buttons (the existing-issues modal after a
// no-match search). Art resolves per the active theme. The marketing tier
// is exported but never rendered anywhere in the app, and no live surface
// renders Detailed without actions or the Section tier at all — see the
// row note.
import { test, expect } from "../../fixtures";
import { serverCreateCycle, serverDeleteCycle, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

// The disabled-feature gate carries each app's own wording: the old
// app's "not enabled" sentence versus the new app's redesigned gate.
// The scenario asserts the capability — a detailed empty state with an
// explanation and a management action — not one app's exact strings.
const CYCLES_OFF_TITLE_ORACLE = "Cycles is not enabled for this project.";
const CYCLES_OFF_TITLE_NEW = "Cycles are turned off for this project.";
const CYCLES_OFF_DESCRIPTION_ORACLE = "Enable the cycles feature";
const CYCLES_OFF_DESCRIPTION_NEW = "switch them on";
const CYCLE_DETAIL_TITLE = "No work items to show in this cycle";
// Same split for the add-existing modal's no-match title.
const MODAL_SIMPLE_TITLE_ORACLE = "No work items found";
const MODAL_SIMPLE_TITLE_NEW = "No results for this search";

// HEAD's serverCreateCycle takes explicit start/end dates (YYYY-MM-DD); cover
// "now" so the cycle reads as current on the detail page.
function cycleDates(): [string, string] {
  const start = new Date();
  const end = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
  return [start.toISOString().slice(0, 10), end.toISOString().slice(0, 10)];
}

test.describe("shared empty-state kit", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  // Three tests, not one: the four steps carry 510s of content polls,
  // which structurally exceeds a single 300s budget once the host is slow
  // (suite17 starved step 3 after slow steps 1-2, so Detailed split from
  // Simple; suite18 then starved the cycle-detail step inside Detailed
  // after an 83s cycles-off step plus a slow hook — the test timeout cut
  // its 90s poll short at 74s). Same split precedent as the 092
  // guest/archived pair.
  test(
    specTitle(["SHELL-104"], "the disabled-feature list renders the detailed tier with themed art"),
    { tag: specTags(["SHELL-104"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const offTitle = driver.target === "web_new" ? CYCLES_OFF_TITLE_NEW : CYCLES_OFF_TITLE_ORACLE;
      const offDescription = driver.target === "web_new" ? CYCLES_OFF_DESCRIPTION_NEW : CYCLES_OFF_DESCRIPTION_ORACLE;
      await test.step("the disabled-feature list renders the detailed tier with an action", async () => {
        await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/cycles`);
        await expect.poll(() => driver.titledEmptyState(offTitle), { timeout: 120_000 }).not.toBeNull();
        const box = await driver.titledEmptyState(offTitle);
        expect(box?.buttons).toContain("Manage features");
        expect(box?.description).toContain(offDescription);
      });

      await test.step("detailed art resolves per the active theme", async () => {
        // The box polls non-null as soon as its title renders, which can
        // precede the themed art resolving — poll for the art itself.
        await expect
          .poll(async () => (await driver.titledEmptyState(offTitle))?.imageSrc, {
            timeout: 60_000,
          })
          .toBeTruthy();
        const box = await driver.titledEmptyState(offTitle);
        const theme = await driver.documentTheme();
        expect(box?.imageSrc).toContain(theme === "dark" ? "dark" : "light");
      });

      await test.step("detailed art follows a live theme switch to dark and back", async () => {
        // The seeded profile is light, so the step above can only ever see
        // the light path. Drive a real switch through the same palette
        // commands SHELL-095 proves on both apps, then restore light so
        // later tests keep a light seed.
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.activatePaletteCommand("Change interface theme");
        await driver.activatePaletteCommand("Dark");
        await expect.poll(() => driver.documentTheme(), { timeout: 120_000 }).toBe("dark");
        await expect
          .poll(async () => (await driver.titledEmptyState(offTitle))?.imageSrc, { timeout: 60_000 })
          .toContain("dark");
        if (!(await driver.isCommandPaletteOpen())) {
          await driver.pressPaletteOpenChord();
          await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        }
        await driver.activatePaletteCommand("Change interface theme");
        await driver.activatePaletteCommand("Light");
        await expect.poll(() => driver.documentTheme(), { timeout: 120_000 }).toBe("light");
        await expect
          .poll(async () => (await driver.titledEmptyState(offTitle))?.imageSrc, { timeout: 60_000 })
          .toContain("light");
      });
    }
  );

  test(
    specTitle(["SHELL-104"], "the fresh cycle detail renders the detailed tier with two actions"),
    { tag: specTags(["SHELL-104"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      await test.step("the fresh cycle detail renders the detailed tier with two actions", async () => {
        const session = await signInSession(seed.email, seed.password);
        const cycleId = await serverCreateCycle(
          seed.workspaceSlug,
          seed.projectId,
          `Parity empty ${Date.now()}`,
          ...cycleDates(),
          session
        );
        try {
          await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`);
          await expect.poll(() => driver.titledEmptyState(CYCLE_DETAIL_TITLE), { timeout: 90_000 }).not.toBeNull();
          const box = await driver.titledEmptyState(CYCLE_DETAIL_TITLE);
          expect(box?.buttons).toContain("Create work item");
          expect(box?.buttons).toContain("Add existing work item");
          // No art assertion here: assetKey-based Detailed boxes render an
          // inline SVG component, not an img; themed art is proven on the
          // cycles-off surface in the test above.
        } finally {
          await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
        }
      });
    }
  );

  test(
    specTitle(["SHELL-104"], "a no-match issue search renders the simple tier with no buttons"),
    { tag: specTags(["SHELL-104"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const modalTitle = driver.target === "web_new" ? MODAL_SIMPLE_TITLE_NEW : MODAL_SIMPLE_TITLE_ORACLE;
      await test.step("a no-match issue search renders the simple tier with no buttons", async () => {
        const session = await signInSession(seed.email, seed.password);
        const cycleId = await serverCreateCycle(
          seed.workspaceSlug,
          seed.projectId,
          `Parity empty ${Date.now()}`,
          ...cycleDates(),
          session
        );
        try {
          await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`);
          await expect.poll(() => driver.titledEmptyState(CYCLE_DETAIL_TITLE), { timeout: 90_000 }).not.toBeNull();
          await driver.clickEmptyStateAction(CYCLE_DETAIL_TITLE, "Add existing work item");
          await driver.typeInIssueSearchModal(`zzz-no-such-issue-${Date.now()}`);
          await expect.poll(() => driver.titledEmptyState(modalTitle), { timeout: 90_000 }).not.toBeNull();
          await expect
            .poll(async () => (await driver.titledEmptyState(modalTitle))?.imageSrc, {
              timeout: 60_000,
            })
            .toBeTruthy();
          const box = await driver.titledEmptyState(modalTitle);
          expect(box?.buttons).toEqual([]);
        } finally {
          await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
        }
      });
    }
  );
});
