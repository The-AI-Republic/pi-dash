// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): edit one's own comment inline, saving
// and discarding. Rows: CMT-003 (inline edit), CMT-012 (edited marker),
// CMT-010 (success notice for update).
import { test, expect } from "../fixtures";
import { composerServerComments, serverIssueIdByName, serverPatchComment, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-003", "CMT-012", "CMT-010"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "edit own comment inline with save and discard"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    const stamp = Date.now();
    const original = `composer edit original ${stamp}`;
    const updated = `composer edit updated ${stamp}`;
    await test.step("sign in, open the work item and post a comment", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
      await driver.composerType(original);
      await driver.composerSubmit();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(original)]));
    });

    await test.step("discarding an edit restores the original body", async () => {
      await driver.composerOpenCommentMenu(original);
      await driver.composerMenuClick("Edit");
      await driver.composerEditType(`unsaved junk ${stamp}`);
      await driver.composerEditDiscard();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(original)]));
    });

    await test.step("a pristine edit draft leaves save disabled", async () => {
      await driver.composerOpenCommentMenu(original);
      await driver.composerMenuClick("Edit");
      // The draft holds the original text, yet save never arms until a
      // keystroke lands: only a content change can be saved through the UI.
      for (let sample = 0; sample < 2; sample += 1) {
        await page.waitForTimeout(5_000);
        const editText =
          (await page
            .locator('div[id^="comment-"] [contenteditable="true"]')
            .innerText()
            .catch(() => "")) ?? "";
        expect(editText).toContain(original);
        expect(await driver.composerEditSaveDisabled()).toBe(true);
      }
      await driver.composerEditDiscard();
    });

    await test.step("re-saving identical html stamps no edit time", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const before = (await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session)).find(
        (comment) => comment.comment_stripped.includes(original)
      );
      expect(before).toBeDefined();
      await serverPatchComment(seed.workspaceSlug, seed.projectId, issueId, before!.id, before!.comment_html, session);
      const after = (await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session)).find(
        (comment) => comment.id === before!.id
      );
      expect(after!.edited_at).toBeNull();
      await expect
        .poll(() => driver.composerCommentMeta(original), { timeout: 30_000 })
        .toMatchObject({
          edited: false,
        });
    });

    await test.step("saving an edit renders the new body with an edited marker", async () => {
      await driver.composerOpenCommentMenu(original);
      await driver.composerMenuClick("Edit");
      await driver.composerEditType(updated);
      await driver.composerEditSave();
      // Notices first: success toasts dismiss within seconds, so the slower
      // render and tooltip reads below would outlive them.
      await expect
        .poll(() => driver.composerVisibleNotices(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ kind: "success" })]));
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(updated)]));
      await expect
        .poll(() => driver.composerCommentMeta(updated), { timeout: 30_000 })
        .toMatchObject({
          edited: true,
        });
    });

    await test.step("the server stored the edit with an edit stamp", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(updated));
      expect(stored).toBeDefined();
      expect(stored!.edited_at).not.toBeNull();
    });
  }
);
