// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the detail sidebar accents an overdue
// due date and notes the cycle the item moved from. Row: ISS-236.
// Edition-gated proof per parity/specs/issues.md §23: the OSS leg lives
// in edition-stubs.spec.ts, this file proves the cloud replacement. It
// runs on the "new" project only — the OSS oracle provably lacks the
// surface. Auth rides API session cookies (the cloud /login is OIDC-only).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddIssuesToCycle,
  serverDeleteCycle,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverPatchIssue,
  serverRemoveIssueFromCycle,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-236"], "cloud: overdue and transfer-hop accents render on the detail"),
  { tag: specTags(["ISS-236"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud badges pin the web_new UI only");
    const tag = `NF984 badges ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      { cycleView: true },
      session
    );
    // Cycles must end in the future: completed cycles reject new issues.
    const previousCycleName = `${tag} previous cycle`;
    const previousCycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      previousCycleName,
      "2027-01-05",
      "2027-01-20",
      session
    );
    const currentCycleName = `${tag} current cycle`;
    const currentCycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      currentCycleName,
      "2027-01-21",
      "2027-02-05",
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // A hop needs a move: attach the issue to the previous cycle, then
    // attach it to the current one (the server transfers it and records
    // the move). Attaching directly would show no note. NB the issues
    // PATCH silently ignores cycle_id; only cycle-issues attaches.
    await serverAddIssuesToCycle(seed.workspaceSlug, projectId, previousCycleId, [issue.id], session);
    await serverAddIssuesToCycle(seed.workspaceSlug, projectId, currentCycleId, [issue.id], session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { target_date: "2020-05-05" }, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the overdue badge accents the due date", async () => {
        const dueText = await driver.propertyValueText("Due date");
        expect(dueText).toContain("May 5, 2020");
        const badge = page.getByTestId("detail-overdue-badge");
        await expect(badge).toBeVisible({ timeout: 60_000 });
        await expect(badge).toContainText(/overdue/i);
      });

      await test.step("the cycle row notes the previous cycle", async () => {
        const hop = page.getByTestId("detail-transfer-hop");
        await expect(hop).toBeVisible({ timeout: 60_000 });
        await expect(hop).toContainText(`Moved from ${previousCycleName}`);
      });
    } finally {
      await serverRemoveIssueFromCycle(seed.workspaceSlug, projectId, currentCycleId, issue.id, session).catch(
        () => {}
      );
      await serverDeleteCycle(seed.workspaceSlug, projectId, previousCycleId, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, projectId, currentCycleId, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
