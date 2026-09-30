// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-113): guest commenting rule. A guest who may
// not use every project feature can only comment on a work item they
// created, unless the project lets guests use everything; a refused post
// stores nothing. Rows: CMT-002.
// Precondition: a fresh seeded stack (parity-up.sh), which provides the
// guest identity and keeps guest_view_all_features off.
import { test, expect } from "../fixtures";
import {
  serverComments,
  serverCreateComment,
  serverCreateCommentRaw,
  serverCreateIssue,
  serverDeleteCommentRaw,
  serverDefaultStateId,
  serverDeleteIssue,
  serverIssues,
  serverPatchProject,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-002"];
const MARKER = "rules cmt-002 guest probe";

test(
  specTitle(ROWS, "guest commenting rule: refusal stores nothing, flag allows"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    if (!seed.guestEmail || !seed.guestPassword)
      throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
    const owner = await signInSessionRetry(seed.email, seed.password);
    const stateId = await serverDefaultStateId(seed.workspaceSlug, seed.projectId, owner);
    const guest = await signInSessionRetry(seed.guestEmail, seed.guestPassword);

    const issueId = await test.step("owner prepares a work item", async () => {
      const id = await serverCreateIssue(
        seed.workspaceSlug,
        seed.projectId,
        owner,
        "Rules CMT-002 guest probe",
        stateId
      );
      const issues = await serverIssues(seed.workspaceSlug, seed.projectId, owner);
      expect(issues.map((i) => i.id)).toContain(id);
      return id;
    });

    await test.step("guest post on another member's item is refused and stores nothing", async () => {
      const before = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
      const refused = await serverCreateCommentRaw(
        seed.workspaceSlug,
        seed.projectId,
        issueId,
        guest,
        `<p>${MARKER} refused</p>`
      );
      expect(refused.status).toBe(400);
      const after = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
      expect(after.map((c) => c.id)).toEqual(before.map((c) => c.id));
      expect(after.some((c) => c.comment_html.includes(MARKER))).toBe(false);
    });

    await test.step("refused guest cannot even open the item in the UI", async () => {
      await driver.rulesEnsureSignedIn(seed.guestEmail ?? "", seed.guestPassword ?? "", seed.workspaceSlug);
      // Someone else's item renders the does-not-exist empty state: no feed,
      // no composer, nothing to post through.
      await driver.rulesOpenIssueDetailRaw(seed.workspaceSlug, seed.projectId, issueId);
      expect(await driver.rulesIssueMissingVisible()).toBe(true);
      expect(await driver.rulesCommentComposerVisible()).toBe(false);
    });

    await test.step("guest post lands once the project lets guests use everything", async () => {
      await serverPatchProject(seed.workspaceSlug, seed.projectId, owner, { guest_view_all_features: true });
      try {
        const created = await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          guest,
          `<p>${MARKER} allowed</p>`
        );
        const listed = await serverComments(seed.workspaceSlug, seed.projectId, issueId, owner);
        expect(listed.map((c) => c.id)).toContain(created.id);
        // The item opens now, with the composer and the guest's card visible.
        await driver.rulesOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
        expect(await driver.rulesCommentComposerVisible()).toBe(true);
        const body = await driver.rulesCommentBodyText(created.id);
        expect(body ?? "").toContain(`${MARKER} allowed`);
        const del = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, issueId, created.id, owner);
        expect(del.status).toBe(204);
      } finally {
        await serverPatchProject(seed.workspaceSlug, seed.projectId, owner, { guest_view_all_features: false });
      }
    });

    await test.step("cleanup removes the probe item", async () => {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, issueId, owner);
      const issues = await serverIssues(seed.workspaceSlug, seed.projectId, owner);
      expect(issues.map((i) => i.id)).not.toContain(issueId);
    });
  }
);
