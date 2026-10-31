// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): project dropdown — the create-issue
// modal's project picker lists joined projects the user may create in,
// selected-first, with search and a "No matching results" empty state;
// picking a project retargets the form so the created issue lands there.
// Observed gaps, asserted as-is (see the inventory row): the multi-select
// "N projects" branch and the current-project exclusion prop have no live
// callers — every caller is single-select and the current project stays
// listed.
// Rows: ISS-211 (project dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverIssues,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-211"], "modal project picker lists, searches and retargets creation"),
  { tag: specTags(["ISS-211"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 proj ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectB = `${tag} B`;
    const projectBId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectB,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const title = `${tag} created`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.issueModalOpenCreate(seed.workspaceSlug, seed.projectId);

      await test.step("the picker preselects the current project, listed first", async () => {
        expect(await driver.issueModalProjectValue()).toContain(seed.projectName);
        await driver.issueModalProjectOpenPicker();
        const options = await driver.issueModalProjectOptionTexts();
        expect(options.some((o) => o.includes(seed.projectName))).toBe(true);
        expect(options.some((o) => o.includes(projectB))).toBe(true);
        // Selected-first sort: the preselected current project leads.
        expect(options[0]).toContain(seed.projectName);
        // No caller passes the current-project exclusion, so the current
        // project stays listed (observed behavior, kept as-is).
        await driver.issueModalProjectPressEscape();
      });

      await test.step("search narrows to the scenario project", async () => {
        await driver.issueModalProjectOpenPicker();
        await driver.issueModalProjectSearch(projectB);
        const options = await driver.issueModalProjectOptionTexts();
        expect(options.some((o) => o.includes(projectB))).toBe(true);
        expect(options.some((o) => o.includes(seed.projectName))).toBe(false);
        await driver.issueModalProjectPressEscape();
      });

      await test.step("a search miss shows the empty message", async () => {
        await driver.issueModalProjectOpenPicker();
        await driver.issueModalProjectSearch("zzz-no-such-project-zzz");
        expect(await driver.issueModalProjectEmptyText()).toBe("No matching results");
        await driver.issueModalProjectPressEscape();
      });

      await test.step("picking retargets the form and creation lands there", async () => {
        await driver.issueModalProjectOpenPicker();
        await driver.issueModalProjectPick(projectB);
        await expect.poll(() => driver.issueModalProjectValue(), { timeout: 15_000 }).toContain(projectB);
        await driver.issueModalFillTitle(title);
        await driver.issueModalSubmit();
        await expect
          .poll(
            async () => (await serverIssues(seed.workspaceSlug, projectBId, session)).some((i) => i.name === title),
            { timeout: 30_000 }
          )
          .toBe(true);
        const inSeed = await serverIssues(seed.workspaceSlug, seed.projectId, session);
        expect(inSeed.some((i) => i.name === title)).toBe(false);
      });
    } finally {
      const created = (await serverIssues(seed.workspaceSlug, projectBId, session)).find((i) => i.name === title);
      if (created) await serverCleanupIssueWithSession(seed.workspaceSlug, projectBId, created.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectBId, session);
    }
  }
);
