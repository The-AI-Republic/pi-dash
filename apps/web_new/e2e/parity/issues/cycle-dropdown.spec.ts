// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): cycle dropdown — project-scoped options,
// "No cycle" clear row, completed-cycle filtering, pick persists. The
// assigned cycle stays listed (the current-cycle exclusion prop has no
// callers); see the final step and the ISS-212 inventory row.
// Rows: ISS-212 (cycle dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverDeleteCycle,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverIssue,
  serverProjectCycles,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

function isoDaysFromNow(days: number): string {
  const d = new Date(Date.now() + days * 86_400_000);
  return d.toISOString();
}

test(
  specTitle(["ISS-212"], "cycle dropdown assigns, clears and filters cycles"),
  { tag: specTags(["ISS-212"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 cycles ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // The Cycle row only renders when the project's cycle view is on, and
    // the seed project keeps it off — so the scenario owns its project.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { cycleView: true },
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // A current cycle to pick and a long-finished one the picker must hide.
    const currentId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      `${tag} current`,
      isoDaysFromNow(-1),
      isoDaysFromNow(30),
      session
    );
    const doneId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      `${tag} done`,
      isoDaysFromNow(-60),
      isoDaysFromNow(-2),
      session
    );
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("empty issues offer the cycle with a No cycle placeholder", async () => {
        await expect.poll(() => driver.propertyValueText("Cycle"), { timeout: 15_000 }).toContain("No cycle");
        const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
        expect(cycles.find((c) => c.id === doneId)?.status.toLowerCase()).toContain("complet");
        await driver.propertyOpenPicker("Cycle");
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes(`${tag} current`))).toBe(true);
        expect(options.some((o) => o.includes(`${tag} done`))).toBe(false);
        await driver.pickerClickOutside();
      });

      await test.step("search narrows options and a miss shows the empty message", async () => {
        await driver.propertyOpenPicker("Cycle");
        await driver.pickerSearch(`${tag} current`);
        expect(await driver.pickerOptionTexts()).toEqual([expect.stringContaining(`${tag} current`)]);
        await driver.pickerSearch(`${tag} no-such-cycle`);
        expect(await driver.pickerOptionTexts()).toEqual([]);
        // The cycle picker carries its own empty message (the module, member,
        // state and priority pickers say "No matching results" instead).
        expect(await driver.pickerEmptyText()).toContain("No matches found");
        await driver.pickerPressEscape();
        await driver.pickerClickOutside();
      });

      await test.step("picking a cycle persists and renders in the row", async () => {
        await driver.propertyOpenPicker("Cycle");
        await driver.pickerPick(`${tag} current`);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).cycle_id, {
            timeout: 15_000,
          })
          .toBe(currentId);
        await expect.poll(() => driver.propertyValueText("Cycle"), { timeout: 15_000 }).toContain(`${tag} current`);
      });

      // Observed behavior: the assigned cycle stays listed (only completed
      // cycles are filtered — the picker's current-cycle exclusion takes a
      // prop no caller passes, so nothing ever filters the assigned cycle).
      await test.step("the assigned cycle stays listed and No cycle clears", async () => {
        await driver.propertyOpenPicker("Cycle");
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes(`${tag} current`))).toBe(true);
        expect(options.some((o) => /no cycle/i.test(o))).toBe(true);
        await driver.pickerPick("No cycle");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).cycle_id, {
            timeout: 15_000,
          })
          .toBe(null);
        await expect.poll(() => driver.propertyValueText("Cycle"), { timeout: 15_000 }).toContain("No cycle");
      });
    } finally {
      // The issue is cycle-free again, so both scenario cycles delete cleanly;
      // the project delete cascades anything left behind.
      await serverDeleteCycle(seed.workspaceSlug, projectId, currentId, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, projectId, doneId, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
