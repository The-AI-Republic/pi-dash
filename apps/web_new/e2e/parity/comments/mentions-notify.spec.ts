// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-115): mentioning a member records the mention
// link on the comment and fans a notification out to the mentioned member.
// Row: CMT-021 (fan-out half; removal is covered by mentions-unmention).
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverDeleteComment,
  serverIssueComments,
  serverUsableSeedIssue,
  serverNotificationsFor,
  serverNotificationsWithSession,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-021"];

test(
  specTitle(ROWS, "mentioning a member links the comment and notifies them"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const bodyText = `action required cmt021a-${Date.now().toString(36)}`;

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

    // Mention notifications persist after their comment is removed, so the
    // spec keys on notification ids unseen at baseline rather than on an
    // empty inbox; reruns on one seeded stack stay green.
    const seenIds = new Set<string>();
    await test.step("record the mentioned member's inbox baseline", async () => {
      const before = await serverNotificationsFor(seed.workspaceSlug, member.email, member.password);
      for (const n of before) seenIds.add(n.id);
    });

    await test.step("post a comment naming the second member", async () => {
      await driver.mentionsPostComment(member.displayName, bodyText);
    });

    let commentId = "";
    await test.step("the server stored the mention link on the comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      const comments = await serverIssueComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const ours = comments.find((c) => c.commentHtml.includes(bodyText));
      expect(ours).toBeDefined();
      commentId = ours!.id;
      expect(ours!.commentHtml).toContain(`entity_identifier="${member.id}"`);
    });

    await test.step("the mentioned member is notified about the issue", async () => {
      // The fan-out runs as a background task, so poll the mentioned
      // member's own inbox until the mention notification lands. One session
      // is reused across iterations (a sibling reseed wipes sessions, so a
      // 401 refreshes it once); signing in per iteration would trip the
      // stack's auth rate limit.
      let memberSession = await signInSession(member.email, member.password);
      const readMentioned = async () => {
        try {
          return await serverNotificationsWithSession(seed.workspaceSlug, memberSession);
        } catch (error) {
          if (!String(error).includes("401") && !String(error).includes("403")) throw error;
          memberSession = await signInSession(member.email, member.password);
          return serverNotificationsWithSession(seed.workspaceSlug, memberSession);
        }
      };
      await expect
        .poll(
          async () => {
            const inbox = await readMentioned();
            return inbox.filter((n) => n.entityIdentifier === issueId && n.isMentioned && !seenIds.has(n.id));
          },
          { timeout: 90_000 }
        )
        .not.toHaveLength(0);
    });

    await test.step("remove the posted comment", async () => {
      const session = await signInSession(seed.email, seed.password);
      await serverDeleteComment(seed.workspaceSlug, seed.projectId, issueId, commentId, session);
    });
  }
);
