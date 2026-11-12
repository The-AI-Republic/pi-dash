// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the timeline draws dependency arrows
// between linked blocks and drag-to-link persists a new edge. Row:
// ISS-237. Edition-gated proof per parity/specs/issues.md §23: the OSS
// leg lives in edition-stubs.spec.ts, this file proves the cloud
// replacement. It runs on the "new" project only — the OSS oracle
// provably lacks the surface. Auth rides API session cookies (the cloud
// /login is OIDC-only).
import { test, expect } from "../fixtures";
import {
  addRelation,
  issueRelations,
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverPatchIssue,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-237"], "cloud: dependency arrows draw and drag-to-link persists"),
  { tag: specTags(["ISS-237"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud dependencies pin the web_new UI only");
    const tag = `NF984 deps ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const firstName = `${tag} first block`;
    const secondName = `${tag} second block`;
    const first = await serverCreateIssueFull(seed.workspaceSlug, projectId, firstName, session);
    const second = await serverCreateIssueFull(seed.workspaceSlug, projectId, secondName, session);
    // Bars must sit inside the timeline's default viewport around today;
    // fixed dates would drift out of it as the suite ages.
    const day = 86_400_000;
    const today = Date.now();
    const dayString = (at: number): string => new Date(at).toISOString().slice(0, 10);
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      first.id,
      { start_date: dayString(today - 7 * day), target_date: dayString(today - 2 * day) },
      session
    );
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      second.id,
      { start_date: dayString(today - 1 * day), target_date: dayString(today + 4 * day) },
      session
    );
    // Seed one edge so the suite asserts both the drawn arrow and a
    // second edge the drag creates.
    await addRelation(seed.workspaceSlug, projectId, second.id, session, {
      relation_type: "finish_after",
      issues: [first.id],
    });
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();
      await driver.ganttOpenTimeline();
      await expect(page.getByRole("link", { name: firstName })).toBeVisible({ timeout: 120_000 });
      await expect(page.getByRole("link", { name: secondName })).toBeVisible({ timeout: 120_000 });

      await test.step("the seeded edge draws one arrow and two handles", async () => {
        await expect.poll(() => driver.ganttDependencyHandleCount(), { timeout: 120_000 }).toBe(2);
        await expect.poll(() => driver.ganttDependencyArrowCount(), { timeout: 120_000 }).toBe(1);
      });

      await test.step("dragging a handle onto a lane links the blocks", async () => {
        await driver.ganttLinkBlocks(second.id, first.id);
        await expect.poll(() => driver.ganttDependencyArrowCount(), { timeout: 120_000 }).toBe(2);
      });

      await test.step("the new edge persists server-side", async () => {
        await expect
          .poll(
            async () =>
              (await issueRelations(seed.workspaceSlug, projectId, first.id, session)).some(
                (row) => row.relation_group === "finish_after" && row.id === second.id
              ),
            { timeout: 30_000 }
          )
          .toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, first.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, second.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
