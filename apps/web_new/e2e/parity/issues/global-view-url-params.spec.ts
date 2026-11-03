// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-122): the global all-issues view ignores URL
// query params in this build — the params are parsed into route filters
// on load, but no layout consumes them, so a filtered-looking deep link
// renders the same unfiltered list. Row: ISS-223 (global view reads URL
// query params into route filters).
//
// Observed behavior notes (the inventory row carries them at update
// time): routeFilters is write-only in this build (parsed in the
// all-issues root, passed to the spreadsheet root which ignores it,
// and to the edition-seam additional layouts which render nothing), so
// ?priority=urgent&order_by=-created_at changes nothing on screen; the
// deep-link filtering the row names, if it exists anywhere, lives
// behind the other-edition layout seam. Not marked as a bug: the seam
// is the deliberate edition boundary.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverIssue,
  serverRequestStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/parity-driver";

/**
 * Open the global view until `issueName` renders, reopening twice: the
 * oracle dev server stalls whole renders under shared-stack load. A
 * genuinely filtered-out row still fails on the final 60s poll.
 */
async function openGlobalSettled(driver: ParityDriver, url: string, issueName: string): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    await driver.openPath(url);
    const settled = await expect
      .poll(() => driver.globalViewIssueVisible(issueName), { timeout: 30_000 })
      .toBe(true)
      .then(
        () => true,
        () => false
      );
    if (settled) return;
  }
  await expect.poll(() => driver.globalViewIssueVisible(issueName), { timeout: 60_000 }).toBe(true);
}

test(
  specTitle(["ISS-223"], "global all-issues view renders unfiltered with and without query params"),
  { tag: specTags(["ISS-223"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 gv${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const urgent = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} urgent`, session);
    const low = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} low`, session);
    try {
      // The priorities genuinely differ server-side, so the param would
      // filter something had the view honored it: the test is not vacuous.
      for (const [id, priority] of [
        [urgent.id, "urgent"],
        [low.id, "low"],
      ] as const) {
        const res = await serverRequestStatus(
          "PATCH",
          `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${id}/`,
          session,
          { priority }
        );
        expect(res.status).toBeGreaterThanOrEqual(200);
        expect(res.status).toBeLessThan(300);
      }
      expect((await serverIssue(seed.workspaceSlug, projectId, urgent.id, session)).priority).toBe("urgent");
      expect((await serverIssue(seed.workspaceSlug, projectId, low.id, session)).priority).toBe("low");

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a filtered-looking deep link still lists both issues", async () => {
        await openGlobalSettled(
          driver,
          `/${seed.workspaceSlug}/workspace-views/all-issues?priority=urgent&order_by=-created_at`,
          `${tag} urgent`
        );
        expect(await driver.globalViewIssueVisible(`${tag} low`)).toBe(true);
      });

      await test.step("the plain view lists the same issues", async () => {
        await openGlobalSettled(driver, `/${seed.workspaceSlug}/workspace-views/all-issues`, `${tag} urgent`);
        expect(await driver.globalViewIssueVisible(`${tag} low`)).toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, urgent.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, low.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
