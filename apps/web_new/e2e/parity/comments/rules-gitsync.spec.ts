// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-113): refuse deletion of a comment kept in sync
// with an external git provider until the repository is unbound. Rows:
// CMT-005.
// Precondition: a fresh seeded stack (parity-up.sh), which binds a
// repository and marks one comment as synced. This scenario consumes that
// fixture (unbind cascades the sync rows, then the comment is deleted), so
// rerunning it needs a reseed.
import { test, expect } from "../fixtures";
import {
  serverComments,
  serverDeleteCommentRaw,
  serverIssues,
  serverUnbindRepository,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-005"];
const MARKER = "parity-git-synced seven";

test(
  specTitle(ROWS, "synced comment delete refuses with conflict until unbind"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const owner = await signInSessionRetry(seed.email, seed.password);

    const { issueId, commentId } = await test.step("locate the seeded synced comment", async () => {
      const issues = await serverIssues(seed.workspaceSlug, seed.projectId, owner);
      for (const issue of issues) {
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, owner);
        const synced = comments.find((c) => c.comment_html.includes(MARKER));
        if (synced) {
          expect(synced.is_synced).toBe(true);
          return { issueId: issue.id, commentId: synced.id };
        }
      }
      throw new Error("[parity] no git-synced comment found; rerun parity-up.sh from a clean checkout.");
    });

    await test.step("sign in and open the work item", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      const body = await driver.rulesCommentBodyText(commentId);
      expect(body ?? "").toContain(MARKER);
    });

    await test.step("delete attempt returns a conflict and the comment stays", async () => {
      const options = await driver.rulesCommentMenuOptions(commentId);
      expect(options).toContain("delete");
      await driver.rulesChooseCommentMenuOption(commentId, "delete");
      await expect
        .poll(
          async () => {
            const toast = await driver.rulesLastToast();
            return toast ? `${toast.title} ${toast.message}`.toLowerCase() : "";
          },
          { timeout: 15_000 }
        )
        .toContain("fail");
      const refused = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, commentId, owner);
      expect(refused.status).toBe(409);
      expect(JSON.stringify(refused.body).toLowerCase()).toContain("unbind");
      const listed = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
      expect(listed.map((c) => c.id)).toContain(commentId);
      const body = await driver.rulesCommentBodyText(commentId);
      expect(body ?? "").toContain(MARKER);
    });

    await test.step("after unbinding the repository the delete succeeds", async () => {
      await serverUnbindRepository(seed.workspaceSlug, seed.projectId, owner);
      const deleted = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, commentId, owner);
      expect(deleted.status).toBe(204);
      const listed = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
      expect(listed.map((c) => c.id)).not.toContain(commentId);
      await driver.rulesReload();
      expect(await driver.rulesCommentCardVisible(commentId)).toBe(false);
    });
  }
);
