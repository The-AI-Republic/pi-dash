// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the sub-issue filter dropdown scopes
// rows by team project. Row: ISS-234 (team-project leg; the type leg
// follows with the types endpoint). Edition-gated proof per
// parity/specs/issues.md §23: the OSS leg lives in edition-stubs.spec.ts,
// this file proves the cloud replacement. It runs on the "new" project
// only — the OSS oracle provably lacks the surface. Auth rides API
// session cookies (the cloud /login is OIDC-only).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddSubIssues,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-234"], "cloud: the team-project filter scopes the sub-issue rows"),
  { tag: specTags(["ISS-234"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud filters pin the web_new UI only");
    const tag = `NF984 teamfilter ${Date.now()}`;
    const projectName = `${tag} project`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const parent = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} parent`, session);
    const childName = `${tag} child`;
    const child = await serverCreateIssueFull(seed.workspaceSlug, projectId, childName, session);
    await serverAddSubIssues(seed.workspaceSlug, projectId, parent.id, [child.id], session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, parent.id);
      const section = page.getByTestId("sub-issues-section");
      await expect(section).toContainText(childName, { timeout: 60_000 });

      await test.step("the dropdown offers the team-projects section", async () => {
        await driver.subIssueFiltersOpen(seed.workspaceSlug, projectId, parent.id);
        await expect.poll(() => driver.subIssueFiltersPanelText(), { timeout: 60_000 }).toContain("Team projects");
      });

      // The section caps visible rows at five with sibling projects around,
      // so each pick searches for its exact project first.
      const teamSection = page.getByTestId("sub-issue-filter-team-project");
      const pickProject = async (name: string): Promise<void> => {
        await teamSection.getByRole("searchbox").fill(name);
        const option = teamSection.getByRole("checkbox", { name });
        await expect(option).toBeVisible({ timeout: 30_000 });
        await option.click();
        await teamSection.getByRole("searchbox").fill("");
      };

      await test.step("picking the child's project keeps the rows", async () => {
        await pickProject(projectName);
        await expect(section).toContainText(childName, { timeout: 30_000 });
        await expect(page.getByTestId("edition-filter-chips")).toContainText(projectName);
      });

      await test.step("picking only another project empties the rows", async () => {
        await pickProject(projectName);
        await pickProject(seed.projectName);
        await expect(section).toContainText("No sub-work items match the filters you've applied.", {
          timeout: 30_000,
        });
        await expect(page.getByTestId("edition-filter-chips")).toContainText(seed.projectName);
      });

      await test.step("removing the chip restores the rows", async () => {
        await page.getByRole("button", { name: `Remove ${seed.projectName} filter` }).click();
        await expect(section).toContainText(childName, { timeout: 30_000 });
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, parent.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, child.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
