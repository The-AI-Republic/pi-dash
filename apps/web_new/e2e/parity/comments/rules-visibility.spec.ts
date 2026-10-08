// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-113): internal/public visibility toggle, badge
// and overflow switch, offered only on externally shared projects. Rows:
// CMT-006.
// Precondition: a fresh seeded stack (parity-up.sh), which publishes the
// seeded project board so the visibility controls render.
import { test, expect } from "../fixtures";
import {
  serverComments,
  serverCreateComment,
  serverCreateIssue,
  serverCreateProject,
  serverDeleteCommentRaw,
  serverDefaultStateId,
  serverDeleteIssue,
  serverDeleteProject,
  serverProjects,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-006"];
const MARKER = "rules cmt-006 visibility probe";

test(
  specTitle(ROWS, "comment visibility toggle, badge and switch"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const owner = await signInSessionRetry(seed.email, seed.password);
    const stateId = await serverDefaultStateId(seed.workspaceSlug, seed.projectId, owner);
    const issueId = await serverCreateIssue(
      seed.workspaceSlug,
      seed.projectId,
      owner,
      "Rules CMT-006 visibility probe",
      stateId
    );
    const comment = await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, owner, `<p>${MARKER}</p>`);

    await test.step("sign in and open the work item", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
    });

    await test.step("new comments start internal with a switch offered", async () => {
      expect(await driver.rulesCommentAccessBadge(comment.id)).toBe("internal");
      const options = await driver.rulesCommentMenuOptions(comment.id);
      expect(options).toContain("access_switch");
    });

    await test.step("switching to public updates badge and server state", async () => {
      await driver.rulesChooseCommentMenuOption(comment.id, "access_switch");
      await expect.poll(() => driver.rulesCommentAccessBadge(comment.id), { timeout: 30_000 }).toBe("public");
      await expect
        .poll(
          async () =>
            (await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner)).find((c) => c.id === comment.id)
              ?.access,
          { timeout: 30_000 }
        )
        .toBe("EXTERNAL");
    });

    await test.step("switching back restores internal on screen and server", async () => {
      await driver.rulesChooseCommentMenuOption(comment.id, "access_switch");
      await expect.poll(() => driver.rulesCommentAccessBadge(comment.id), { timeout: 30_000 }).toBe("internal");
      await expect
        .poll(
          async () =>
            (await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner)).find((c) => c.id === comment.id)
              ?.access,
          { timeout: 30_000 }
        )
        .toBe("INTERNAL");
    });

    await test.step("a project without external sharing offers no visibility controls", async () => {
      // Deleting a project leaves its identifier row behind, so a fixed
      // identifier collides on the second run against any stack (the create
      // commits the project, then refuses the duplicate identifier with
      // 400). Suffix both per run; the sweep below stays prefix-based to
      // clear crashed runs' projects. Base-36 upper fits the identifier
      // charset (≤12 chars, no special characters).
      const runSuffix = Date.now().toString(36).toUpperCase().slice(-6).padStart(6, "0");
      const stale = await serverProjects(seed.workspaceSlug, owner);
      for (const p of stale.filter((p) => p.identifier.startsWith("RLS")))
        await serverDeleteProject(seed.workspaceSlug, p.id, owner);
      const plainId = await serverCreateProject(
        seed.workspaceSlug,
        owner,
        `Rules scratch ${runSuffix}`,
        `RLS${runSuffix}`
      );
      const plain = await serverProjects(seed.workspaceSlug, owner);
      expect(plain.find((p) => p.id === plainId)?.anchor).toBe(null);
      const plainState = await serverDefaultStateId(seed.workspaceSlug, plainId, owner);
      const plainIssue = await serverCreateIssue(
        seed.workspaceSlug,
        plainId,
        owner,
        "Rules CMT-006 plain probe",
        plainState
      );
      const plainComment = await serverCreateComment(
        seed.workspaceSlug,
        plainId,
        plainIssue,
        owner,
        `<p>${MARKER} plain</p>`
      );
      await driver.rulesOpenIssueDetail(seed.workspaceSlug, plainId, plainIssue);
      expect(await driver.rulesCommentBodyText(plainComment.id)).toContain(`${MARKER} plain`);
      expect(await driver.rulesCommentAccessBadge(plainComment.id)).toBe("hidden");
      const options = await driver.rulesCommentMenuOptions(plainComment.id);
      expect(options).not.toContain("access_switch");
      const delComment = await serverDeleteCommentRaw(seed.workspaceSlug, plainId, plainIssue, plainComment.id, owner);
      expect(delComment.status).toBe(204);
      await serverDeleteIssue(seed.workspaceSlug, plainId, plainIssue, owner);
      await serverDeleteProject(seed.workspaceSlug, plainId, owner);
    });

    await test.step("cleanup removes the probe comment and item", async () => {
      const del = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, comment.id, owner);
      expect(del.status).toBe(204);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, issueId, owner);
    });
  }
);
