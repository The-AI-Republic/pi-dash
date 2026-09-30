// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-115): removing a mention from a comment clears
// its link, so no stale mention source remains on the stored comment.
// Row: CMT-021 (removal half; fan-out is covered by mentions-notify).
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverDeleteComment,
  serverIssueComments,
  serverUsableSeedIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-021"];

test(
  specTitle(ROWS, "removing a mention clears its link on the comment"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const bodyText = `draft note cmt021b-${Date.now().toString(36)}`;
    const plainText = `final note cmt021b-${Date.now().toString(36)}`;

    await test.step("sign in through the UI", async () => {
      await driver.mentionsEnsureSignedIn(seed.email, seed.password);
    });

    let issueId = "";
    await test.step("open a seeded issue", async () => {
      const session = await signInSession(seed.email, seed.password);
      // Select by seeded name (probe issues from sibling runs make position
      // unstable) and skip issues triaged into intake, which redirect away
      // from the comment composer.
      const target = await serverUsableSeedIssue(seed.workspaceSlug, seed.projectId, seed.issueNames, session);
      issueId = target.id;
      await driver.mentionsOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
    });

    await test.step("post a comment naming the second member", async () => {
      await driver.mentionsPostComment(member.displayName, bodyText);
    });

    let commentId = "";
    await test.step("the stored comment carries the mention link", async () => {
      const session = await signInSession(seed.email, seed.password);
      const comments = await serverIssueComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const ours = comments.find((c) => c.commentHtml.includes(bodyText));
      expect(ours).toBeDefined();
      commentId = ours!.id;
      expect(ours!.commentHtml).toContain(`entity_identifier="${member.id}"`);
    });

    await test.step("edit the comment to remove the mention", async () => {
      await driver.mentionsEditRemovingMention(bodyText, plainText);
    });

    await test.step("the stored comment no longer links the member", async () => {
      const session = await signInSession(seed.email, seed.password);
      const comments = await serverIssueComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const ours = comments.find((c) => c.id === commentId);
      expect(ours).toBeDefined();
      expect(ours!.commentHtml).not.toContain("mention-component");
      expect(ours!.commentHtml).not.toContain(`entity_identifier="${member.id}"`);
      expect(ours!.commentHtml).toContain(plainText);
    });

    await test.step("remove the posted comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      await serverDeleteComment(seed.workspaceSlug, seed.projectId, issueId, commentId, session);
    });
  }
);
