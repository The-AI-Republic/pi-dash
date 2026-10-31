// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-116): one reaction per person per emoji —
// repeating the same emoji does not duplicate the row, the server refuses
// the duplicate with a client error, counts never double-count, and the
// refused repeat surfaces a failure notice. Row: CMT-024.
import { test, expect } from "../fixtures";
import {
  serverAddCommentReactionRaw,
  serverAddIssueReactionRaw,
  serverCleanupIssueWithSession,
  serverCommentReactions,
  serverCreateIssueFull,
  serverIssueReactions,
  serverPostComment,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(["CMT-024"], "repeating an issue reaction is refused without duplicating"),
  { tag: specTags(["CMT-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF116 dupi ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
      // The trigger renders disabled while metadata hydrates; wait for
      // the enabled control before touching the picker.
      await expect.poll(() => driver.reactionsIssuePickerReady(), { timeout: 30_000 }).toBe(true);
      const picked = await driver.issueAddReaction();
      await expect
        .poll(() => driver.issueReactionChips(), { timeout: 15_000 })
        .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);

      await test.step("a direct repeat POST is refused with a client error", async () => {
        const repeat = await serverAddIssueReactionRaw(
          seed.workspaceSlug,
          seed.projectId,
          issue.id,
          session,
          picked.code
        );
        expect(repeat.status).toBe(400);
        expect(repeat.body).toContain("error");
      });

      await test.step("the row stays single in the UI and on the server", async () => {
        expect(await driver.issueReactionChips()).toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        const reactions = await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("re-adding a server-side reaction surfaces a failure notice", async () => {
        // Clear the chip, re-add behind the client's back so its store goes
        // stale, then add the same emoji through the picker: the client
        // POSTs a duplicate, the server refuses, and the UI reports it.
        await driver.issueClickReactionChip(picked.emoji);
        await expect.poll(() => driver.issueReactionChips(), { timeout: 15_000 }).toEqual([]);
        const seeded = await serverAddIssueReactionRaw(
          seed.workspaceSlug,
          seed.projectId,
          issue.id,
          session,
          picked.code
        );
        expect(seeded.status).toBe(201);
        await driver.reactionsIssueAddEmoji(picked.emoji);
        await expect.poll(() => driver.sawToast("Reaction creation failed"), { timeout: 15_000 }).toBe(true);
        // Re-open so the detail refetches: exactly one row must render.
        await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        await expect
          .poll(() => driver.issueReactionChips(), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        const reactions = await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(reactions).toHaveLength(1);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["CMT-024"], "repeating a comment reaction is refused without duplicating"),
  { tag: specTags(["CMT-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF116 dupc ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const body = `${tag} body`;
      const posted = await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
      const picked = await driver.activityAddCommentReaction(body);
      await expect
        .poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 })
        .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);

      await test.step("a direct repeat POST is refused with a client error", async () => {
        const repeat = await serverAddCommentReactionRaw(
          seed.workspaceSlug,
          seed.projectId,
          posted.id,
          session,
          picked.code
        );
        expect(repeat.status).toBe(400);
        expect(repeat.body).toContain("Reaction already exists");
      });

      await test.step("the row stays single in the UI and on the server", async () => {
        expect(await driver.activityCommentReactionChips(body)).toEqual([
          { emoji: picked.emoji, count: 1, reacted: true },
        ]);
        const reactions = await serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("re-adding a server-side reaction surfaces a failure notice", async () => {
        await driver.activityClickCommentReactionChip(body, picked.emoji);
        await expect.poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 }).toEqual([]);
        const seeded = await serverAddCommentReactionRaw(
          seed.workspaceSlug,
          seed.projectId,
          posted.id,
          session,
          picked.code
        );
        expect(seeded.status).toBe(201);
        await driver.reactionsCommentAddEmoji(body, picked.emoji);
        await expect.poll(() => driver.sawToast("Reaction creation failed"), { timeout: 15_000 }).toBe(true);
        // Re-open so the card refetches: exactly one row must render.
        await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        await expect
          .poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        const reactions = await serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session);
        expect(reactions).toHaveLength(1);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
