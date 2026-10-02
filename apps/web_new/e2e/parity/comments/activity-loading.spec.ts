// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-114): feed loading and empty states. While
// the history reads are in flight the section shows a skeleton loader and
// no entries; once they resolve the entries appear. An item whose history
// reads resolve empty renders no rows and no errors — the empty read is
// produced by answering the history endpoints with empty lists, which is
// the only way to observe the client's empty branch (every persisted item
// carries at least its creation entry). Row: CMT-016.
import { test, expect } from "../fixtures";
import { serverCleanupIssue, serverCreateIssue, signInSessionWithRetry } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-016"];

test(
  specTitle(ROWS, "activity skeleton shows while entries load"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Loading probe ${Date.now()}`
    );
    try {
      await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.page.route("**/history/**", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 2500));
        await route.continue();
      });
      await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      let sawLoader = false;
      await expect
        .poll(
          async () => {
            if (await driver.activityLoadingVisible()) sawLoader = true;
            return (await driver.activityEntryTexts()).length;
          },
          { timeout: 180_000, intervals: [250] }
        )
        .toBeGreaterThan(0);
      expect(sawLoader).toBe(true);
      expect(await driver.activityLoadingVisible()).toBe(false);
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);

test(
  specTitle(ROWS, "activity empty feed renders no rows and no errors"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Empty probe ${Date.now()}`
    );
    try {
      await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.page.route("**/history/**", async (route) => {
        await route.fulfill({ status: 200, contentType: "application/json", body: "[]" });
      });
      await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      await expect.poll(() => driver.activityLoadingVisible(), { timeout: 60_000 }).toBe(false);
      expect(await driver.activityEntryTexts()).toEqual([]);
      // No error surface: the section heading still renders and the page
      // shows no alert.
      await expect(driver.page.getByText("Activity", { exact: true }).first()).toBeVisible();
      expect(await driver.page.getByRole("alert").count()).toBe(0);
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);
