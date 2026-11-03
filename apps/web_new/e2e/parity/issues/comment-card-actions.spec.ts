// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): comment card actions — access toggle,
// fold/unfold and copy link with hash highlight.
// Rows: ISS-200 (access), ISS-201 (fold), ISS-202 (copy link).
import { test, expect } from "../fixtures";
import {
  commentText,
  parityProjectIdentifier,
  serverComments,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverPostComment,
  serverPublishBoard,
  serverUnpublishBoard,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

async function openOwnIssue(
  driver: {
    openEntry(): Promise<void>;
    signInWithPassword(e: string, p: string): Promise<void>;
    openIssueDetail(w: string, p: string, i: string): Promise<void>;
  },
  seed: { email: string; password: string; workspaceSlug: string; projectId: string },
  issueId: string,
  projectId?: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openIssueDetail(seed.workspaceSlug, projectId ?? seed.projectId, issueId);
}

test(
  specTitle(["ISS-200"], "toggle a comment between private and public"),
  { tag: specTags(["ISS-200"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 access ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // The access switch renders only for anchored projects, so the scenario
    // owns its project and publishes that project's public board itself —
    // the board is always owned by this run, never reused, so the final
    // unpublish step always runs (no conditional skip).
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const body = `${tag} body`;
    await serverPostComment(seed.workspaceSlug, projectId, issue.id, `<p>${body}</p>`, session);
    const board = await serverPublishBoard(seed.workspaceSlug, projectId, session);
    expect(board.owned).toBe(true);
    try {
      await openOwnIssue(driver, seed, issue.id, projectId);

      const accessOf = async (): Promise<string> => {
        const comments = await serverComments(seed.workspaceSlug, projectId, issue.id, session);
        return comments.find((c) => commentText(c.comment_html).includes(body))?.access ?? "";
      };

      await test.step("menu offers the public switch on the anchored project", async () => {
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).toContain("Switch to public comment");
      });

      await test.step("switching flips the access server-side and the menu label", async () => {
        await driver.activityClickMenuItem("Switch to public comment");
        await expect.poll(() => driver.sawToast("Comment updated successfully"), { timeout: 15_000 }).toBe(true);
        await expect.poll(accessOf, { timeout: 15_000 }).toBe("EXTERNAL");
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).toContain("Switch to private comment");
      });

      await test.step("switching back restores private", async () => {
        await driver.activityClickMenuItem("Switch to private comment");
        await expect.poll(accessOf, { timeout: 15_000 }).toBe("INTERNAL");
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).toContain("Switch to public comment");
      });

      await test.step("unpublishing hides the switch again", async () => {
        await serverUnpublishBoard(seed.workspaceSlug, projectId, board.boardId, session);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).not.toContain("Switch to public comment");
        expect(await driver.activityMenuItems()).not.toContain("Switch to private comment");
      });
    } finally {
      if (board.owned) await serverUnpublishBoard(seed.workspaceSlug, projectId, board.boardId, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(specTitle(["ISS-201"], "fold and unfold a comment"), { tag: specTags(["ISS-201"]) }, async ({ driver, seed }) => {
  const tag = `NF122 fold ${Date.now()}`;
  const session = await signInSession(seed.email, seed.password);
  const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
  try {
    const body = `${tag} noisy`;
    await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
    await openOwnIssue(driver, seed, issue.id);

    const labelsOf = async (): Promise<string[]> => {
      const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
      return comments.find((c) => commentText(c.comment_html).includes(body))?.labels ?? [];
    };

    await test.step("folding hides the body behind an expand toggle", async () => {
      await driver.activityOpenCommentMenu(body);
      expect(await driver.activityMenuItems()).toContain("Fold comment");
      await driver.activityClickMenuItem("Fold comment");
      await expect.poll(() => labelsOf(), { timeout: 15_000 }).toContain("fold");
      await expect.poll(() => driver.activityCommentBodyVisible(body), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the in-card toggle expands without touching labels", async () => {
      await driver.activityExpandFoldedComment(body);
      await expect.poll(() => driver.activityCommentBodyVisible(body), { timeout: 15_000 }).toBe(true);
      expect(await labelsOf()).toContain("fold");
    });

    await test.step("unfolding from the menu clears the marker", async () => {
      await driver.activityOpenCommentMenu(body);
      expect(await driver.activityMenuItems()).toContain("Unfold comment");
      await driver.activityClickMenuItem("Unfold comment");
      await expect.poll(() => labelsOf(), { timeout: 15_000 }).not.toContain("fold");
      await expect.poll(() => driver.activityCommentBodyVisible(body), { timeout: 30_000 }).toBe(true);
    });
  } finally {
    await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
  }
});

test(
  specTitle(["ISS-202"], "copy a comment link and follow its highlight"),
  { tag: specTags(["ISS-202"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 link ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const body = `${tag} anchor`;
      const posted = await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
      await openOwnIssue(driver, seed, issue.id);
      let link = "";

      await test.step("copying yields a deep link with toast", async () => {
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).toContain("Copy link");
        link = await driver.activityCopyCommentLink(body);
        expect(link).toContain(`#comment-${posted.id}`);
        await expect.poll(() => driver.sawToast("Comment link copied to clipboard"), { timeout: 15_000 }).toBe(true);
      });

      await test.step("following the link highlights the comment", async () => {
        await page.goto(link);
        await expect.poll(() => driver.activityCommentHighlighted(body), { timeout: 30_000 }).toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
