// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): keyboard submit rules shared by the
// composer and the inline edit form. Row: CMT-011.
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import { composerServerComments, serverIssueIdByName, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-011"];
const uploadFile = fileURLToPath(new URL("./composer-upload.png", import.meta.url));

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "Enter posts, Shift+Enter newlines, Ctrl/Cmd+Enter inert, empty never submits"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    const stamp = Date.now();
    const marker = `composer keyboard ${stamp}`;
    await test.step("sign in and open the work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
      expect(await driver.composerSubmitDisabled()).toBe(true);
    });

    await test.step("Enter on an empty composer is a no-op", async () => {
      await driver.composerPressEnter();
      expect(await driver.composerVisibleCommentTexts()).not.toEqual(
        expect.arrayContaining([expect.stringContaining(marker)])
      );
    });

    await test.step("Ctrl+Enter and Cmd+Enter neither post nor break the line", async () => {
      await driver.composerType(marker);
      const before = await driver.composerDraftText();
      const editor = page.getByRole("group", { name: "Add comment" }).locator('[contenteditable="true"]');
      await editor.focus();
      await page.keyboard.press("Control+Enter");
      await page.waitForTimeout(1_000);
      expect(await driver.composerDraftText()).toBe(before);
      await page.keyboard.press("Meta+Enter");
      await page.waitForTimeout(1_000);
      expect(await driver.composerDraftText()).toBe(before);
      expect(await driver.composerVisibleCommentTexts()).not.toEqual(
        expect.arrayContaining([expect.stringContaining(marker)])
      );
    });

    await test.step("Shift+Enter inserts a newline without posting", async () => {
      await driver.composerPressShiftEnter();
      await driver.composerType(`second line ${stamp}`);
      expect(await driver.composerDraftText()).toContain(marker);
      expect(await driver.composerVisibleCommentTexts()).not.toEqual(
        expect.arrayContaining([expect.stringContaining(marker)])
      );
    });

    await test.step("Enter posts the two-line draft as one comment", async () => {
      await driver.composerPressEnter();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
      const posted = (await driver.composerVisibleCommentTexts()).find((body) => body.includes(marker));
      expect(posted).toContain(`second line ${stamp}`);
    });

    await test.step("Enter in the edit form saves", async () => {
      await driver.composerOpenCommentMenu(marker);
      await driver.composerMenuClick("Edit");
      await driver.composerEditType(`${marker} via enter`);
      await driver.composerEditPressEnter();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(`${marker} via enter`)]));
    });

    await test.step("the server stored exactly one comment for the draft", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      // The edit-form step above replaced the two-line body, so the stored
      // row carries the final text; the two-line post itself was asserted in
      // the feed right after Enter.
      const matches = comments.filter((comment) => comment.comment_stripped.includes(marker));
      expect(matches).toHaveLength(1);
      expect(matches[0]!.comment_stripped).toContain(`${marker} via enter`);
    });
  }
);

test(
  specTitle(ROWS, "Enter mid-upload is a no-op until the upload lands"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    const marker = `composer midupload ${Date.now()}`;
    await test.step("sign in and open the work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
    });

    await test.step("Enter while an upload is stalled neither posts nor drops the draft", async () => {
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      await page.route("**/api/assets/v2/**", async (route) => {
        await gate;
        // The recovery navigation below can invalidate the held request
        // first; a late continue is then a no-op, never a failure.
        await route.continue().catch(() => undefined);
      });
      try {
        await driver.composerType(marker);
        await driver.composerAttachFile(uploadFile);
        await page.waitForTimeout(2_000);
        expect(await driver.composerSubmitDisabled()).toBe(true);
        await driver.composerPressEnter();
        await page.waitForTimeout(2_000);
        expect(await driver.composerVisibleCommentTexts()).not.toEqual(
          expect.arrayContaining([expect.stringContaining(marker)])
        );
        expect(await driver.composerDraftText()).toContain(marker);
      } finally {
        release();
        // Let the held upload settle before navigating away: tearing the
        // route down mid-flight detaches the frame.
        await page.waitForTimeout(2_000);
        await page.unrouteAll({ behavior: "wait" });
      }
      // A stalled upload never recovers its node (see NEWFRONT-145), so
      // reopen for a clean composer and prove Enter still posts after it.
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
      const clean = `${marker} clean`;
      await driver.composerType(clean);
      await driver.composerPressEnter();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(clean)]));
    });
  }
);
