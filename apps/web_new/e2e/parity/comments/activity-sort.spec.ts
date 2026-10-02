// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-114): feed sort order. Flipping the control
// reverses the merged feed immediately, moves the composer from below the
// feed (oldest-first) to above it (newest-first), and the choice survives a
// reload through browser-local storage. Row: CMT-014.
import { test, expect } from "../fixtures";
import { serverCreateIssue, serverCleanupIssue, serverHistory, signInSessionWithRetry } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-014"];

test(
  specTitle(ROWS, "activity sort flips order, moves the composer, and persists"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const stamp = Date.now();
    const commentBody = `sort probe comment ${stamp}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `Sort probe ${stamp}`);
    try {
      await test.step("sign in and open the fresh work item", async () => {
        await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      });

      await test.step("oldest-first by default with the composer below the feed", async () => {
        await driver.activityComposerType(commentBody);
        await driver.activityComposerSubmit();
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(2);
        expect(await driver.activityComposerPosition()).toBe("below");
      });
      const oldestFirst = await driver.activityEntryTexts();
      expect(oldestFirst[1]).toContain(commentBody);

      await test.step("flip to newest-first: order reverses, composer moves above", async () => {
        await driver.activityToggleSort();
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toEqual([...oldestFirst].reverse());
        expect(await driver.activityComposerPosition()).toBe("above");
        expect(JSON.parse((await driver.activityStoredSort()) ?? "null")).toBe("desc");
      });

      await test.step("the choice survives a reload", async () => {
        await driver.page.reload();
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 180_000 }).toEqual([...oldestFirst].reverse());
        expect(await driver.activityComposerPosition()).toBe("above");
        expect(JSON.parse((await driver.activityStoredSort()) ?? "null")).toBe("desc");
      });

      await test.step("the server order is unchanged (sorting is client-side)", async () => {
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
        expect(props.status).toBe(200);
        expect(comments.status).toBe(200);
        expect(props.entries).toHaveLength(1);
        expect(comments.entries).toHaveLength(1);
      });
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);
