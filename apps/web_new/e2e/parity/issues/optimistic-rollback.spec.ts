// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-122): a property edit applies
// optimistically and, when the server rejects it, visibly snaps back
// to the prior value while the server row stays untouched; the next
// save goes through normally. Row: ISS-225 (optimistic mutation with
// rollback-on-error).
//
// Observed behavior notes (the inventory row carries them at update
// time): the store applies the patch to the shared issue map and the
// list before the PATCH resolves, and on rejection reverts both from
// the pre-edit snapshot and rethrows; the row requires the visible
// snap-back only (no notice is asserted). The failure here is injected
// once at the network layer with a delay so the optimistic value stays
// observable; the route removes itself before answering.
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

test(
  specTitle(["ISS-225"], "a rejected priority edit snaps back, then saves normally on retry"),
  { tag: specTags(["ISS-225"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 or${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const res = await serverRequestStatus(
      "PATCH",
      `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/`,
      session,
      { priority: "low" }
    );
    expect(res.status).toBeGreaterThanOrEqual(200);
    expect(res.status).toBeLessThan(300);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 30_000 }).toContain("Low");

      await test.step("the pick applies optimistically, then snaps back on rejection", async () => {
        await driver.failNextIssuePatch(500, 1500);
        await driver.propertyOpenPicker("Priority");
        await driver.pickerPick("Urgent");
        await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 30_000 }).toContain("Urgent");
        await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 30_000 }).toContain("Low");
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).priority).toBe("low");
      });

      await test.step("the retry saves through to the server", async () => {
        await driver.propertyOpenPicker("Priority");
        await driver.pickerPick("Urgent");
        await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 30_000 }).toContain("Urgent");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).priority, {
            timeout: 30_000,
          })
          .toBe("urgent");
      });
    } finally {
      await driver.clearIssuePatchFailure().catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
