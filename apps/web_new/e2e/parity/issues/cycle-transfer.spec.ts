// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-122): the cycle page offers a transfer
// prompt when the cycle holds transferable issues, and the transfer
// modal moves them to another incomplete cycle with a success notice.
// Row: ISS-224 (cycle transfer-issues affordance).
//
// Observed behavior notes (the inventory row carries them at update
// time): the prompt renders only for completed cycles (status is
// date-driven: a past end date reads COMPLETED) and then only when the
// progress snapshot is empty and backlog+unstarted+started exceeds
// zero; the transferable counts come only from the progress endpoint;
// the modal lists the project's incomplete cycles with a search box,
// and picking one transfers with "Success! / Work items have been
// transferred successfully", then refetches both cycles. The transfer
// freezes the source's pre-move counts into its progress snapshot and
// the progress endpoint serves snapshot counts once one exists, so the
// source keeps reporting its old total; the prompt button likewise
// stays up since the refetch carries no snapshot.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAttachCycleIssues,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCycleDetail,
  serverCycleProgress,
  serverIssue,
  serverPatchCycle,
  serverProjectCycles,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/parity-driver";

/**
 * Open the cycle page until the transfer prompt renders, reopening
 * twice: the oracle dev server stalls whole renders under
 * shared-stack load. A genuinely absent prompt still fails the final
 * 60s poll.
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
  specTitle(["ISS-224"], "cycle transfer prompt moves remaining issues to another cycle"),
  { tag: specTags(["ISS-224"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 ct${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const sourceName = `${tag} source`;
    const targetName = `${tag} target`;
    // The source starts current (attaching to a completed cycle is a
    // 400), takes the issue, then backdates into COMPLETED — the
    // prompt's render gate. The target stays current so the modal lists it.
    const sourceId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      sourceName,
      "2026-09-01",
      "2026-12-31",
      session
    );
    const targetId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      targetName,
      "2026-09-01",
      "2026-12-31",
      session
    );
    await serverAttachCycleIssues(seed.workspaceSlug, projectId, sourceId, [issue.id], session);
    await serverPatchCycle(
      seed.workspaceSlug,
      projectId,
      sourceId,
      { start_date: "2026-01-01", end_date: "2026-02-01" },
      session
    );
    try {
      // Preconditions, server-side: the source reads COMPLETED with an
      // empty snapshot and really holds one transferable (backlog)
      // issue before the UI is even opened.
      const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, session);
      expect(cycles.find((row) => row.id === sourceId)?.status).toBe("COMPLETED");
      const before = await serverCycleProgress(seed.workspaceSlug, projectId, sourceId, session);
      expect(before.backlog + before.unstarted + before.started).toBeGreaterThan(0);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the cycle page offers the transfer prompt", async () => {
        await openCycleSettled(driver, seed.workspaceSlug, projectId, sourceId);
      });

      await test.step("the modal lists the target cycle and the transfer lands", async () => {
        await driver.cycleTransferOpen();
        await expect
          .poll(async () => (await driver.cycleTransferOptionNames()).some((text) => text.includes(targetName)), {
            timeout: 30_000,
          })
          .toBe(true);
        await driver.cycleTransferPick(targetName);
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 30_000 }
          )
          .toContain("Work items have been transferred successfully");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).cycle_id, {
            timeout: 30_000,
          })
          .toBe(targetId);
        expect((await serverCycleProgress(seed.workspaceSlug, projectId, targetId, session)).total).toBe(1);
        // The transfer freezes the source's pre-move counts into its
        // progress snapshot, and the progress endpoint serves snapshot
        // counts once one exists — so the source keeps reporting 1.
        // (The prompt button likewise stays up: the progress refetch
        // carries no snapshot, so the store still reads it as empty.)
        await expect
          .poll(async () => (await serverCycleDetail(seed.workspaceSlug, projectId, sourceId, session)).snapshotEmpty, {
            timeout: 30_000,
          })
          .toBe(false);
        expect((await serverCycleProgress(seed.workspaceSlug, projectId, sourceId, session)).total).toBe(1);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
