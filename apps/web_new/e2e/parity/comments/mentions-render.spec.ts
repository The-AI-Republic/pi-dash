// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-115): mentions render inside saved comments as
// identifiable member references attributed to the mentioned user.
// Row: CMT-020.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverDeleteComment,
  serverIssueComments,
  serverUsableSeedIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-020"];

test(
  specTitle(ROWS, "saved comments render mentions as member references"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const bodyText = `please review this cmt020-${Date.now().toString(36)}`;

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

    await test.step("the feed shows the mention as a reference to that member", async () => {
      const refs = await driver.mentionsVisibleReferences();
      const match = refs.find((ref) => ref.text === `@${member.displayName}`);
      expect(match).toBeDefined();
      expect(match!.href).toBe(`/${seed.workspaceSlug}/profile/${member.id}`);
    });

    let commentId = "";
    await test.step("the server stored the mention markup on the comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      const comments = await serverIssueComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const ours = comments.find((c) => c.commentHtml.includes(bodyText));
      expect(ours).toBeDefined();
      commentId = ours!.id;
      expect(ours!.commentHtml).toContain('entity_name="user_mention"');
      expect(ours!.commentHtml).toContain(`entity_identifier="${member.id}"`);
    });

    await test.step("remove the posted comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      await serverDeleteComment(seed.workspaceSlug, seed.projectId, issueId, commentId, session);
    });
  }
);
