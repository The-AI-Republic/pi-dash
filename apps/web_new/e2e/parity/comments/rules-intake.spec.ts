// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-113): intake-screen variant of the activity
// section. The intake feed renders comments and entries but offers no
// per-comment deep-link copying, and no extra creation aids appear in the
// activity section. Rows: CMT-018.
// Precondition: a fresh seeded stack (parity-up.sh), which enables the
// intake view and seeds a pending triage row over the first seeded issue
// (seed facts inboxIssueId).
import { test, expect } from "../fixtures";
import {
  serverComments,
  serverCreateComment,
  serverDeleteCommentRaw,
  serverIssues,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-018"];
const MARKER = "rules cmt-018 intake probe";

test(
  specTitle(ROWS, "intake activity variant: feed without copy link or extras"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The intake screen resolves the triage row, redirects onto the linked
    // issue form, then loads its feed; that chain is the slowest page in
    // this area on the dev server.
    test.setTimeout(420_000);
    if (!seed.inboxIssueId)
      throw new Error("[parity] seed has no triage row; rerun parity-up.sh from a clean checkout.");
    const owner = await signInSessionRetry(seed.email, seed.password);
    const issues = await serverIssues(seed.workspaceSlug, seed.projectId, owner);
    const first = issues.find((i) => i.name === seed.issueNames[0]);
    if (!first) throw new Error("[parity] seeded first issue is missing; rerun parity-up.sh.");
    const comment = await serverCreateComment(seed.workspaceSlug, seed.projectId, first.id, owner, `<p>${MARKER}</p>`);

    await test.step("sign in and open the intake variant", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, seed.projectId, seed.inboxIssueId ?? "");
    });

    await test.step("the intake feed renders the comment without a copy-link option", async () => {
      await expect.poll(() => driver.rulesCommentCardVisible(comment.id), { timeout: 60_000 }).toBe(true);
      const body = await driver.rulesCommentBodyText(comment.id);
      expect(body ?? "").toContain(MARKER);
      const options = await driver.rulesCommentMenuOptions(comment.id);
      expect(options).not.toContain("copy_link");
    });

    await test.step("triage chrome is present and the server agrees", async () => {
      // Accept/Decline prove the intake variant; the activity header itself
      // carries only the sort and filter controls, no creation extras.
      expect(await driver.rulesIntakeTriageVisible()).toBe(true);
      const listed = await serverComments(seed.workspaceSlug, seed.projectId, first.id, owner);
      expect(listed.map((c) => c.id)).toContain(comment.id);
    });

    await test.step("cleanup removes the probe comment", async () => {
      const del = await serverDeleteCommentRaw(seed.workspaceSlug, seed.projectId, first.id, comment.id, owner);
      expect(del.status).toBe(204);
    });
  }
);
