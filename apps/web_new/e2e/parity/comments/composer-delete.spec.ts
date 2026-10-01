// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): delete one's own comment from the
// overflow menu. Rows: CMT-004 (delete), CMT-010 (success notice).
import { test, expect } from "../fixtures";
import { serverComments, serverIssueIdByName, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-004", "CMT-010"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "delete own comment from the overflow menu"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const marker = `composer delete ${Date.now()}`;
    await test.step("sign in, open the work item and post a comment", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-2");
      await driver.composerType(marker);
      await driver.composerSubmit();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
    });

    await test.step("delete removes the card and reports success", async () => {
      await driver.composerOpenCommentMenu(marker);
      await driver.composerMenuClick("Delete");
      // Notices first: success toasts dismiss within seconds, so the slower
      // disappearance read below would outlive them.
      await expect
        .poll(() => driver.composerVisibleNotices(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ kind: "success" })]));
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .not.toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
    });

    await test.step("the server no longer stores the comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[1]!, session);
      const comments = await serverComments(seed.workspaceSlug, seed.projectId, issueId, session);
      expect(comments.some((comment) => comment.comment_stripped.includes(marker))).toBe(false);
    });
  }
);
