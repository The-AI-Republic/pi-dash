// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-122): worklog surfaces stay absent in OSS.
// Row: ISS-206 (worklog absent).
import { test, expect } from "../fixtures";
import { serverCreateIssueFull, serverCleanupIssueWithSession, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-206"], "no worklog surfaces render in the OSS build"),
  { tag: specTags(["ISS-206"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 worklog ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);

      await test.step("no worklog button in the activity header", async () => {
        expect(await driver.worklogCreateVisible()).toBe(false);
      });

      await test.step("no worklog rows, summary or filter category", async () => {
        const bodyText = await page.locator("body").innerText();
        expect(bodyText).not.toMatch(/work\s?log/i);
        const options = await driver.activityFilterOptions();
        expect(options.map((o) => o.label)).toEqual(["Updates", "Comments", "State", "Assignee"]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
