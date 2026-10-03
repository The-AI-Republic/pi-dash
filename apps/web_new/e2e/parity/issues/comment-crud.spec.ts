// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): comment create, edit and delete.
// Rows: ISS-197 (add), ISS-198 (edit), ISS-199 (delete).
import { test, expect } from "../fixtures";
import {
  commentText,
  serverComments,
  serverCreateIssueFull,
  serverCleanupIssueWithSession,
  serverPostComment,
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
  issueId: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
}

test(
  specTitle(["ISS-197"], "add a comment through the composer"),
  { tag: specTags(["ISS-197"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 add ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await openOwnIssue(driver, seed, issue.id);
      const body = `${tag} hello`;

      await test.step("empty submit is guarded", async () => {
        expect(await driver.activityComposerText()).toBe("");
        expect(await driver.activityCommentTexts()).toEqual([]);
      });

      await test.step("enter posts with toast, reset and server state", async () => {
        await driver.activityPostComment(body);
        await expect.poll(() => driver.sawToast("Comment created successfully"), { timeout: 15_000 }).toBe(true);
        expect(await driver.activityComposerText()).toBe("");
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(comments).toHaveLength(1);
        expect(commentText(comments[0]!.comment_html)).toContain(body);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-198"], "edit a comment inline with cancel and save paths"),
  { tag: specTags(["ISS-198"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 edit ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const original = `${tag} original`;
      await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${original}</p>`, session);
      await openOwnIssue(driver, seed, issue.id);

      await test.step("author-only menu offers edit", async () => {
        await driver.activityOpenCommentMenu(original);
        expect(await driver.activityMenuItems()).toContain("Edit");
      });

      await test.step("cancel restores the original body", async () => {
        await driver.activityClickMenuItem("Edit");
        await driver.activityCancelEdit(original);
        expect(await driver.activityCommentBodyVisible(original)).toBe(true);
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(comments[0]!.edited_at).toBeNull();
      });

      await test.step("save renames with an edited marker and server state", async () => {
        const updated = `${tag} updated`;
        await driver.activityEditComment(original, updated);
        expect(await driver.activityCommentBodyVisible(updated)).toBe(true);
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(comments).toHaveLength(1);
        expect(commentText(comments[0]!.comment_html)).toContain(updated);
        expect(comments[0]!.edited_at).not.toBeNull();
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-199"], "delete a comment without confirmation"),
  { tag: specTags(["ISS-199"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 delete ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const body = `${tag} doomed`;
      const posted = await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
      await openOwnIssue(driver, seed, issue.id);

      await test.step("delete removes immediately with toast and server state", async () => {
        await driver.activityOpenCommentMenu(body);
        expect(await driver.activityMenuItems()).toContain("Delete");
        await driver.activityClickMenuItem("Delete");
        // No confirmation dialog exists: the card detaches with no further
        // clicks, which a modal would stall past this timeout.
        await expect.poll(() => driver.activityCommentTexts(), { timeout: 10_000 }).toEqual([]);
        await expect.poll(() => driver.sawToast("Comment removed successfully"), { timeout: 15_000 }).toBe(true);
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(comments.find((c) => c.id === posted.id)).toBeUndefined();
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
