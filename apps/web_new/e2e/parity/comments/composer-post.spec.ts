// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): draft and post a comment through the
// rich-text composer, including pasted formatted content. Rows: CMT-001
// (composer post incl. paste), CMT-010 (success notice for create).
import { test, expect } from "../fixtures";
import { composerServerComments, serverIssueIdByName, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-001", "CMT-010"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "draft and post a comment with pasted rich text"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const marker = `composer post ${Date.now()}`;
    await test.step("sign in and open the seeded work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
    });

    await test.step("draft text and paste a formatted fragment", async () => {
      await driver.composerType(marker);
      await driver.composerPasteHtml("<p>pasted <strong>bold</strong> fragment</p>");
      expect(await driver.composerDraftText()).toContain(marker);
      expect(await driver.composerSubmitDisabled()).toBe(false);
    });

    await test.step("post through the submit button", async () => {
      await driver.composerSubmit();
      // Notices first: success toasts dismiss within seconds, so the render
      // read below would outlive them.
      await expect
        .poll(() => driver.composerVisibleNotices(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ kind: "success" })]));
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
    });

    await test.step("the composer clears for the next comment", async () => {
      expect(await driver.composerDraftText()).toBe("");
    });

    await test.step("the server stored the comment with its formatted body", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(marker));
      expect(stored).toBeDefined();
      expect(stored!.comment_html).toContain("bold");
      expect(stored!.actorDisplayName.length).toBeGreaterThan(0);
      const visible = await driver.composerVisibleCommentTexts();
      expect(visible.some((body) => body.includes(marker) && body.includes("bold"))).toBe(true);
    });
  }
);
