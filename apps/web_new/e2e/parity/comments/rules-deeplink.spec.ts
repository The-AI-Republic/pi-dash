// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-113): copy a deep link to a single comment and
// open it — the link carries a comment anchor, opening it scrolls the feed
// to that comment and highlights it briefly. Rows: CMT-007. (Hidden on
// intake items: covered by the CMT-018 scenario.)
import { test, expect } from "../fixtures";
import {
  serverComments,
  serverCreateComment,
  serverCreateIssue,
  serverDeleteCommentRaw,
  serverDefaultStateId,
  serverDeleteIssue,
  serverIssueDetail,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-007"];
const MARKER = "rules cmt-007 deep link probe";

test(
  specTitle(ROWS, "copy comment deep link; opening scrolls and highlights"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const owner = await signInSessionRetry(seed.email, seed.password);
    const stateId = await serverDefaultStateId(seed.workspaceSlug, seed.projectId, owner);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      owner,
      "Rules CMT-007 deep link probe",
      stateId
    );
    const comment = await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, owner, `<p>${MARKER}</p>`);
    const detail = await serverIssueDetail(seed.workspaceSlug, seed.projectId, issueId, owner);

    await test.step("sign in and open the work item", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
    });

    const copied = await test.step("copy the comment link", async () => {
      const options = await driver.rulesCommentMenuOptions(comment.id);
      expect(options).toContain("copy_link");
      await driver.rulesChooseCommentMenuOption(comment.id, "copy_link");
      await expect
        .poll(
          async () => {
            const toast = await driver.rulesLastToast();
            return toast ? `${toast.title} ${toast.message}`.toLowerCase() : "";
          },
          { timeout: 15_000 }
        )
        .toContain("copied");
      const text = await driver.rulesReadClipboard();
      const expected = `/${seed.workspaceSlug}/browse/${detail.project_identifier}-${detail.sequence_id}/#comment-${comment.id}`;
      expect(text).toContain(expected);
      return text;
    });

    await test.step("opening the link scrolls to the comment and highlights it", async () => {
      await driver.rulesOpenDeepLink(copied);
      await expect.poll(() => driver.rulesCommentHighlighted(comment.id), { timeout: 30_000 }).toBe(true);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
      const listed = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
      expect(listed.map((c) => c.id)).toContain(comment.id);
    });

    await test.step("cleanup removes the probe comment and item", async () => {
      const del = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, comment.id, owner);
      expect(del.status).toBe(204);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, issueId, owner);
    });
  }
);
