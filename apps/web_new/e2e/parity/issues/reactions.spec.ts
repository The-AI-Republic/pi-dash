// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): emoji reactions on comments and issues.
// Rows: ISS-203 (comment reactions), ISS-204 (issue reactions).
import { test, expect } from "../fixtures";
import {
  serverCommentReactions,
  serverCreateIssueFull,
  serverCleanupIssueWithSession,
  serverIssueReactions,
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
  specTitle(["ISS-203"], "react to a comment with toggle, count and tooltip"),
  { tag: specTags(["ISS-203"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 reactc ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const body = `${tag} body`;
      const posted = await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
      await openOwnIssue(driver, seed, issue.id);

      await test.step("adding shows a highlighted chip with count and server state", async () => {
        expect(await driver.activityCommentReactionChips(body)).toEqual([]);
        const picked = await driver.activityAddCommentReaction(body);
        await expect
          .poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        await expect.poll(() => driver.sawToast("Reaction created successfully"), { timeout: 15_000 }).toBe(true);
        const reactions = await serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("the chip tooltip names the reactor", async () => {
        const picked = (await driver.activityCommentReactionChips(body))[0]!;
        const tooltip = await driver.activityChipTooltipText(body, picked.emoji, "Parity Oracle");
        expect(tooltip).toContain("Parity Oracle");
      });

      await test.step("clicking the chip again removes the reaction", async () => {
        const picked = (await driver.activityCommentReactionChips(body))[0]!;
        await driver.activityClickCommentReactionChip(body, picked.emoji);
        await expect.poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 }).toEqual([]);
        await expect
          .poll(() => serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session), {
            timeout: 15_000,
          })
          .toEqual([]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-204"], "react to the issue with toggle and count"),
  { tag: specTags(["ISS-204"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 reacti ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await openOwnIssue(driver, seed, issue.id);

      await test.step("the add control shows with no reactions", async () => {
        expect(await driver.issueReactionChips()).toEqual([]);
        const server = await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(server).toEqual([]);
      });

      await test.step("adding shows a highlighted chip with count and server state", async () => {
        const picked = await driver.issueAddReaction();
        await expect
          .poll(() => driver.issueReactionChips(), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        const reactions = await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("clicking the chip again removes the reaction", async () => {
        const picked = (await driver.issueReactionChips())[0]!;
        await driver.issueClickReactionChip(picked.emoji);
        await expect.poll(() => driver.issueReactionChips(), { timeout: 15_000 }).toEqual([]);
        await expect
          .poll(() => serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session), {
            timeout: 15_000,
          })
          .toEqual([]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
