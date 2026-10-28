// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the modal type picker and the
// sub-issue type filter section. Rows: ISS-129 (modal type half),
// ISS-234 (type options half). Edition-gated proof per
// parity/specs/issues.md §23: the OSS legs live in create-modal.spec.ts
// and edition-stubs.spec.ts, this file proves the cloud replacement. It
// runs on the "new" project only — the OSS oracle provably lacks the
// surface. Auth rides API session cookies (the cloud /login is
// OIDC-only). Row-scoping and the detail badge wait on the read-side
// type grant (NEWFRONT-1 process note): no live read exposes the type
// yet, so these legs prove the picker, the create payload and the
// filter plumbing.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddSubIssues,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateIssueType,
  serverCreateProjectWithFlags,
  serverIssues,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-129"], "cloud: the modal offers work-item types and files the pick"),
  { tag: specTags(["ISS-129"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud modal pins the web_new UI only");
    const tag = `NF984 modtype ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const bug = await serverCreateIssueType(seed.workspaceSlug, projectId, "Bug", session);
    const issueName = `${tag} typed`;
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();
      await driver.openCreateModal();

      await test.step("the picker offers the project type", async () => {
        await driver.modalSelectIssueType("Bug");
      });

      await test.step("the create payload carries the pick", async () => {
        await driver.fillCreateTitle(issueName);
        const [response] = await Promise.all([
          page.waitForResponse((entry) => entry.url().includes("/issues/") && entry.request().method() === "POST", {
            timeout: 60_000,
          }),
          driver.submitCreateModal(),
        ]);
        expect(response.request().postDataJSON()?.type).toBe(bug.id);
        await expect
          .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
          .toEqual(expect.arrayContaining([issueName]));
      });
    } finally {
      const rows = await serverIssues(seed.workspaceSlug, projectId, session).catch(() => []);
      const created = rows.find((row) => row.name === issueName);
      if (created !== undefined) {
        await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, created.id, session).catch(() => {});
      }
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-234"], "cloud: the type filter section offers the project types"),
  { tag: specTags(["ISS-234"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud filters pin the web_new UI only");
    const tag = `NF984 typefilter ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const parent = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} parent`, session);
    const child = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} child`, session);
    await serverAddSubIssues(seed.workspaceSlug, projectId, parent.id, [child.id], session);
    await serverCreateIssueType(seed.workspaceSlug, projectId, "Bug", session);
    await serverCreateIssueType(seed.workspaceSlug, projectId, "Spike", session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, parent.id);
      const section = page.getByTestId("sub-issues-section");
      await expect(section).toContainText(`${tag} child`, { timeout: 60_000 });

      await test.step("the dropdown offers the type section with both types", async () => {
        await driver.subIssueFiltersOpen(seed.workspaceSlug, projectId, parent.id);
        await expect.poll(() => driver.subIssueFiltersPanelText(), { timeout: 60_000 }).toContain("Work item type");
        const typeSection = page.getByTestId("sub-issue-filter-type");
        await expect(typeSection).toContainText("Bug", { timeout: 30_000 });
        await expect(typeSection).toContainText("Spike", { timeout: 30_000 });
      });

      await test.step("picking a type raises its chip", async () => {
        const typeSection = page.getByTestId("sub-issue-filter-type");
        await typeSection.getByRole("checkbox", { name: "Bug" }).click();
        await expect(page.getByTestId("edition-filter-chips")).toContainText("Bug");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, parent.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, child.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
