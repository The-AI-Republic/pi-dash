// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-113): collapse and expand a long comment. Any
// viewer may toggle it; the collapsed state hides the body and its
// reactions until expanded, and persists across reloads. Rows: CMT-008.
import { test, expect } from "../fixtures";
import {
  serverAddCommentReaction,
  serverComments,
  serverCreateComment,
  serverCreateIssue,
  serverDeleteCommentRaw,
  serverDefaultStateId,
  serverDeleteIssue,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-008"];
const MARKER = "rules cmt-008 collapse probe";
const REACTION = "127881";

test(
  specTitle(ROWS, "collapse and expand a comment; state persists"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const owner = await signInSessionRetry(seed.email, seed.password);
    const stateId = await serverDefaultStateId(seed.workspaceSlug, seed.projectId, owner);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      owner,
      "Rules CMT-008 collapse probe",
      stateId
    );
    const comment = await serverCreateComment(
      seed.workspaceSlug,
      seed.projectId,
      issueId,
      owner,
      `<p>${MARKER} with a long body that a viewer may fold away</p>`
    );
    await serverAddCommentReaction(seed.workspaceSlug, seed.projectId, comment.id, owner, REACTION);

    await test.step("sign in and open the work item", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
    });

    await test.step("folding hides the body and reactions, and stores the fold marker", async () => {
      const options = await driver.rulesCommentMenuOptions(comment.id);
      expect(options).toContain("fold");
      await driver.rulesChooseCommentMenuOption(comment.id, "fold");
      await expect.poll(() => driver.rulesCommentBodyText(comment.id), { timeout: 30_000 }).toBe(null);
      const cardText = await driver.page.locator(`#comment-${comment.id}`).innerText();
      expect(cardText).not.toContain(MARKER);
      expect(cardText).not.toContain("🎉");
      await expect
        .poll(
          async () =>
            (await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner)).find((c) => c.id === comment.id)
              ?.labels ?? [],
          { timeout: 30_000 }
        )
        .toContain("fold");
      const reopened = await driver.rulesCommentMenuOptions(comment.id);
      expect(reopened).toContain("unfold");
    });

    await test.step("the collapsed state survives a reload", async () => {
      await driver.rulesReload();
      expect(await driver.rulesCommentBodyText(comment.id)).toBe(null);
    });

    await test.step("expanding reveals the body and clears the marker", async () => {
      await driver.rulesChooseCommentMenuOption(comment.id, "unfold");
      await expect.poll(() => driver.rulesCommentBodyText(comment.id), { timeout: 30_000 }).not.toBe(null);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
      const cardText = await driver.page.locator(`#comment-${comment.id}`).innerText();
      expect(cardText).toContain("🎉");
      await expect
        .poll(
          async () =>
            (await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner)).find((c) => c.id === comment.id)
              ?.labels ?? [],
          { timeout: 30_000 }
        )
        .not.toContain("fold");
    });

    await test.step("cleanup removes the probe comment and item", async () => {
      const del = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, comment.id, owner);
      expect(del.status).toBe(204);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, issueId, owner);
    });
  }
);
