// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-112): attach image files inside the comment
// composer and post; a replace-edit drops the image cleanly. Rows:
// CMT-009 (attachments bind to draft / attach on post), CMT-010
// (success notices for the post), CMT-001 (post with files).
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import { composerServerComments, serverIssueIdByName, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-009", "CMT-010"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);
const uploadFile = fileURLToPath(new URL("./composer-upload.png", import.meta.url));

// Oracle truth for NEWFRONT-145: a failed attachment upload surfaces NO
// notice at all and leaves a stuck 0%-progress image node in the draft;
// submit usually re-arms, but intermittently stays disabled until reload
// (flaky even holding the steps fixed, so only the stable half — silent,
// draft kept — is asserted). Intended per CMT-009/CMT-010: failed uploads
// report an error without losing the draft; every upload resolves in
// exactly one visible notice of the matching kind. Locked here so the fix
// has a regression test.
test(
  specTitle(ROWS, "bug: failed attachment upload stays silent without losing the draft (NEWFRONT-145)"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    await test.step("sign in and open the work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-3");
    });

    await test.step("a failed upload stays silent and keeps the draft", async () => {
      let uploadAttempts = 0;
      await page.route("**/api/assets/v2/**", (route) => {
        uploadAttempts += 1;
        void route.abort();
      });
      const draft = `composer upload keeps draft ${Date.now()}`;
      await driver.composerType(draft);
      await driver.composerAttachFile(uploadFile);
      // Give the upload chain time to settle, then assert the stable oracle
      // outcome: the upload was attempted, still no notice (success or
      // error), and the draft text is intact.
      await page.waitForTimeout(8_000);
      expect(uploadAttempts).toBeGreaterThan(0);
      expect(await driver.composerVisibleNotices()).toEqual([]);
      expect(await driver.composerDraftText()).toContain(draft);
      await page.unroute("**/api/assets/v2/**");
    });
  }
);

// Oracle truth for NEWFRONT-147: opening the edit form on an image comment
// leaves save disabled indefinitely — the embedded image node keeps the
// editor's upload flag stuck, so an edit that keeps the image can never
// be saved (only removing the image re-arms save). Intended per CMT-009:
// editing keeps attachments bound, duplicating them into the new
// revision. Locked here so the fix has a regression test.
test(
  specTitle(ROWS, "bug: edit that keeps the image never arms save (NEWFRONT-147)"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    const marker = `composer imgsave ${Date.now()}`;
    await test.step("sign in, open the work item and post an image comment", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-3");
      await driver.composerType(marker);
      await driver.composerAttachFile(uploadFile);
      await expect.poll(() => driver.composerSubmitDisabled(), { timeout: 90_000 }).toBe(false);
      await driver.composerSubmit();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
      await expect.poll(() => driver.composerCommentImageCount(marker), { timeout: 60_000 }).toBeGreaterThan(0);
    });

    await test.step("save stays disabled while the image is in the edit draft", async () => {
      await driver.composerOpenCommentMenu(marker);
      await driver.composerMenuClick("Edit");
      // The draft is non-empty (marker text plus the image node), yet save
      // never arms; sample repeatedly so a slow-but-working flow would show.
      for (let sample = 0; sample < 3; sample += 1) {
        await page.waitForTimeout(5_000);
        const editText =
          (await page
            .locator('div[id^="comment-"] [contenteditable="true"]')
            .innerText()
            .catch(() => "")) ?? "";
        expect(editText).toContain(marker);
        expect(await driver.composerEditSaveDisabled()).toBe(true);
      }
    });
  }
);

const FLOW_ROWS = ["CMT-001", "CMT-009", "CMT-010"];

test(
  specTitle(FLOW_ROWS, "attach a file in the composer and post; a replace-edit drops it cleanly"),
  { tag: specTags(FLOW_ROWS) },
  async ({ driver, seed }) => {
    const marker = `composer attachment ${Date.now()}`;
    await test.step("sign in and open the work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-3");
    });

    await test.step("attach a file to the draft and post", async () => {
      await driver.composerType(marker);
      await driver.composerAttachFile(uploadFile);
      await expect.poll(() => driver.composerSubmitDisabled(), { timeout: 90_000 }).toBe(false);
      await driver.composerSubmit();
      // Notices first: success toasts dismiss within seconds, so the slower
      // render reads below would outlive them.
      await expect
        .poll(() => driver.composerVisibleNotices(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ kind: "success" })]));
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
      // Attached images render as embedded image nodes in the card body;
      // the card shows no file-name list.
      await expect.poll(() => driver.composerCommentImageCount(marker), { timeout: 60_000 }).toBeGreaterThan(0);
    });

    await test.step("the server stored the post with its embedded image", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[2]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(marker));
      expect(stored).toBeDefined();
      expect(stored!.comment_html).toContain("image-component");
      expect(stored!.comment_html).toContain('status="uploaded"');
    });

    await test.step("a replace-edit drops the image and saves the new text", async () => {
      await driver.composerOpenCommentMenu(marker);
      await driver.composerMenuClick("Edit");
      await driver.composerEditType(`${marker} edited`);
      await driver.composerEditSave();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(`${marker} edited`)]));
      await expect.poll(() => driver.composerCommentImageCount(`${marker} edited`), { timeout: 60_000 }).toBe(0);
    });

    await test.step("the server stored the text-only edit with an edit stamp", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[2]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(marker));
      expect(stored).toBeDefined();
      expect(stored!.comment_stripped).toContain(`${marker} edited`);
      expect(stored!.comment_html).not.toContain("image-component");
      expect(stored!.edited_at).not.toBeNull();
    });
  }
);
