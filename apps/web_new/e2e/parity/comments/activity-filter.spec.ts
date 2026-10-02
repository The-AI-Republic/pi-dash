// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-114): feed filter menu. The menu offers the
// updates/comments/state/assignee categories, shows a marker on the control
// while the selection is narrowed, hides unchecked categories from the
// feed, and refuses to remove the last active filter. Filtering is
// client-side over the merged feed; the server state is untouched. Row:
// CMT-015.
import { test, expect } from "../fixtures";
import { serverCreateIssue, serverCleanupIssue, serverHistory, signInSessionWithRetry } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-015"];

test(
  specTitle(ROWS, "activity filter narrows the feed with a marker and keeps one filter"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const stamp = Date.now();
    const commentBody = `filter probe comment ${stamp}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Filter probe ${stamp}`
    );
    try {
      await test.step("sign in and open the fresh work item", async () => {
        await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
        await driver.activityComposerType(commentBody);
        await driver.activityComposerSubmit();
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(2);
      });

      await test.step("the menu offers all four categories with no marker while full", async () => {
        await driver.activityOpenFilterMenu();
        expect(await driver.activityFilterOptionLabels()).toEqual(["Updates", "Comments", "State", "Assignee"]);
        expect(await driver.activityFilterNarrowed()).toBe(false);
      });

      await test.step("unchecking comments hides them and raises the marker", async () => {
        await driver.activityOpenFilterMenu();
        await driver.activityToggleFilterOption("Comments");
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(1);
        const texts = await driver.activityEntryTexts();
        expect(texts[0]).not.toContain(commentBody);
        expect(await driver.activityFilterNarrowed()).toBe(true);
        const stored = await driver.activityStoredFilters();
        expect(stored).not.toBeNull();
        expect(JSON.parse(stored ?? "[]")).not.toContain("COMMENT");
      });

      await test.step("the last active filter cannot be removed", async () => {
        for (const label of ["State", "Assignee", "Updates"]) {
          await driver.activityOpenFilterMenu();
          await driver.activityToggleFilterOption(label);
        }
        // Updates was the last one standing: the control refuses, so the
        // feed and the stored selection are unchanged.
        expect(await driver.activityEntryTexts()).toHaveLength(1);
        expect(await driver.activityFilterNarrowed()).toBe(true);
        expect(JSON.parse((await driver.activityStoredFilters()) ?? "[]")).toEqual(["ACTIVITY"]);
      });

      await test.step("re-checking comments brings them back", async () => {
        await driver.activityOpenFilterMenu();
        await driver.activityToggleFilterOption("Comments");
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(2);
        expect(await driver.activityEntryTexts()).toEqual(
          expect.arrayContaining([expect.stringContaining(commentBody)])
        );
      });

      await test.step("the server state is untouched by filtering", async () => {
        const props = await serverHistory(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          session,
          "?activity_type=issue-property"
        );
        const comments = await serverHistory(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          session,
          "?activity_type=issue-comment"
        );
        expect(props.entries).toHaveLength(1);
        expect(comments.entries).toHaveLength(1);
      });
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);
