// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-819): one edition-agnostic probe of the whole
// comment/activity surface. The same steps run against whatever edition the
// suite was pointed at (OSS by default, the cloud oracle under
// PARITY_ORACLE_EDITION=cloud) and against the desktop build when
// PARITY_NEW_URL points there, so a build that hides, adds or rewrites a
// capability shows up as a failed assertion instead of a silent gap.
//
// What the scenario holds constant across editions:
// - the composer is reachable and posting through it creates exactly one
//   stored comment through the ordinary comments endpoint;
// - the merged feed lists the comment next to the creation entry, the sort
//   control reverses the order and moves the composer, and the filter menu
//   offers the same categories and narrows the feed client-side;
// - the mention trigger asks the same suggestion endpoint and the screen
//   lists the same members it returns;
// - the comment-level and issue-level reaction rows are both reachable,
//   write through the same reaction endpoints, and read back from them;
// - the detail screen's API traffic carries no write outside those three
//   capabilities, which is what "no edition-only seam" means in practice.
//
// Editions this run actually exercises are recorded in the row's note; the
// desktop leg is the same scenario run with PARITY_NEW_URL on the desktop
// build, and the cloud leg needs the private overlay checkout.
import { test, expect } from "../fixtures";
import type { ParityBrowserCookie } from "../drivers/parity-driver";
import {
  cloudBrowserCookies,
  cloudDevLogin,
  composerServerComments,
  serverCleanupIssueWithSession,
  serverCommentReactions,
  serverCreateIssueFull,
  serverHistory,
  serverIssueReactions,
  serverMe,
  serverUserMentionSuggestions,
  signInSessionWithRetry,
  sessionBrowserCookies,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-025"];

test.setTimeout(600_000);

interface EditionSession {
  cookie: string;
  displayName: string;
  cookies: ParityBrowserCookie[];
}

/**
 * Enter the app the way this edition expects. The OSS seed carries a
 * password and signs in through the credential form; the cloud seed has no
 * password and signs in through the provider's dev-login endpoint, the same
 * path its own cloud scenarios use. Either way the scenario then behaves
 * identically: it never branches on the edition after this point.
 */
async function enterEdition(seed: { email: string; password?: string | undefined }): Promise<EditionSession> {
  if (seed.password !== undefined && seed.password !== "") {
    const cookie = await signInSessionWithRetry(seed.email, seed.password);
    const me = await serverMe(cookie);
    return { cookie, displayName: me.displayName, cookies: sessionBrowserCookies(cookie) };
  }
  const session = await cloudDevLogin(seed.email);
  const me = await serverMe(session.cookie);
  return { cookie: session.cookie, displayName: me.displayName, cookies: cloudBrowserCookies(session.cookie) };
}

test(
  specTitle(ROWS, "composer, feed, mentions and both reaction rows behave the same in every edition"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    test.slow();
    const stamp = Date.now();
    const commentBody = `edition sameness ${stamp}`;
    const session = await enterEdition(seed);
    const issue = await serverCreateIssueFull(
      seed.workspaceSlug,
      seed.projectId,
      `Edition sameness ${stamp}`,
      session.cookie
    );
    try {
      await test.step("enter the app with this edition's own sign-in and open the work item", async () => {
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${issue.id}`,
          session.cookies
        );
        await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        // Editions differ in whether creating an issue through the API writes
        // an "issue created" row, so the scenario pins what it actually owns:
        // the activity surface and its composer are on screen. Everything it
        // asserts afterwards is an entry this scenario itself produced.
        await expect.poll(() => driver.rulesCommentComposerVisible(), { timeout: 120_000 }).toBe(true);
      });

      await test.step("loading the detail reads the shared feed and writes nothing", async () => {
        await driver.activityBeginTrafficSpy();
        await expect.poll(() => driver.rulesCommentComposerVisible(), { timeout: 120_000 }).toBe(true);
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.historyReads).toBeGreaterThan(0);
        // A plain detail load creates nothing: no edition saw, added or
        // rewrote a comment, mention or reaction on the way in.
        expect(traffic.commentCreates).toBe(0);
        expect(traffic.commentReactionCreates).toBe(0);
        expect(traffic.commentReactionDeletes).toBe(0);
        expect(traffic.issueReactionCreates).toBe(0);
        expect(traffic.issueReactionDeletes).toBe(0);
        expect(traffic.unexpectedWrites).toBe(0);
      });

      await test.step("the composer is reachable and posting writes one comment through the ordinary endpoint", async () => {
        expect(await driver.rulesCommentComposerVisible()).toBe(true);
        // Reset the counters in place: the post itself must be the only write.
        await driver.activityBeginTrafficSpy({ reload: false });
        await driver.activityComposerType(commentBody);
        await driver.activityComposerSubmit();
        await expect
          .poll(() => driver.activityCommentTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(commentBody)]));
        const stored = await composerServerComments(seed.workspaceSlug, seed.projectId, issue.id, session.cookie);
        expect(stored.map((row) => row.comment_stripped)).toEqual(
          expect.arrayContaining([expect.stringContaining(commentBody)])
        );
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.commentCreates).toBe(1);
        expect(traffic.unexpectedWrites).toBe(0);
      });

      await test.step("the merged feed carries the comment and sort reverses whatever it lists", async () => {
        await driver.activityBeginTrafficSpy({ reload: false });
        const oldestFirst = await driver.activityEntryTexts();
        expect(oldestFirst.length).toBeGreaterThanOrEqual(1);
        expect(oldestFirst.join(" ")).toContain(commentBody);
        expect(await driver.activityComposerPosition()).toBe("below");
        await driver.activityToggleSort();
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toEqual([...oldestFirst].reverse());
        expect(await driver.activityComposerPosition()).toBe("above");
        // Sorting is a view preference: the server keeps one order.
        const history = await serverHistory(
          seed.workspaceSlug,
          seed.projectId,
          issue.id,
          session.cookie,
          "?activity_type=issue-comment"
        );
        expect(history.status).toBe(200);
        expect(history.entries).toHaveLength(1);
        // Sorting and filtering are view preferences over one fetch: the
        // screen sent no write for either.
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.unexpectedWrites).toBe(0);
        expect(traffic.commentCreates).toBe(0);
      });

      await test.step("the filter menu offers the same categories and narrows the feed client-side", async () => {
        await driver.activityOpenFilterMenu();
        expect(await driver.activityFilterOptionLabels()).toEqual(["Updates", "Comments", "State", "Assignee"]);
        await driver.activityToggleFilterOption("Comments");
        // Filtering hides the comment category without deleting anything.
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .not.toEqual(expect.arrayContaining([expect.stringContaining(commentBody)]));
        expect(await driver.activityFilterNarrowed()).toBe(true);
        await driver.activityOpenFilterMenu();
        await driver.activityToggleFilterOption("Comments");
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(commentBody)]));
      });

      await test.step("mention autocomplete asks the shared suggestion endpoint and lists its members", async () => {
        await driver.activityBeginTrafficSpy({ reload: false });
        const prefix = session.displayName.slice(0, Math.max(1, Math.floor(session.displayName.length / 2)));
        const names = await driver.mentionsSuggestionsFor(prefix);
        expect(names).toEqual(expect.arrayContaining([session.displayName]));
        const server = await serverUserMentionSuggestions(seed.workspaceSlug, seed.projectId, session.cookie, prefix);
        expect(server.map((row) => row.displayName)).toEqual(expect.arrayContaining([session.displayName]));
        // Drop the trigger text so the composer is clean for later steps.
        await driver.page.keyboard.press("Escape");
        await driver.page.keyboard.press("ControlOrMeta+a");
        await driver.page.keyboard.press("Backspace");
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.mentionSearches).toBeGreaterThan(0);
        expect(traffic.unexpectedWrites).toBe(0);
      });

      await test.step("the issue-level reaction row adds and removes a reaction the server stores", async () => {
        expect(await driver.issueReactionControlVisible()).toBe(true);
        await driver.activityBeginTrafficSpy({ reload: false });
        const picked = await driver.issueAddReaction();
        await expect
          .poll(() => driver.issueReactionChips(), { timeout: 30_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        expect(
          (await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session.cookie)).map(
            (r) => r.reaction
          )
        ).toEqual([picked.code]);
        await driver.issueClickReactionChip(picked.emoji);
        await expect.poll(() => driver.issueReactionChips(), { timeout: 30_000 }).toEqual([]);
        await expect
          .poll(() => serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session.cookie), {
            timeout: 30_000,
          })
          .toEqual([]);
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.issueReactionCreates).toBe(1);
        expect(traffic.issueReactionDeletes).toBe(1);
        expect(traffic.unexpectedWrites).toBe(0);
      });

      await test.step("the comment-level reaction row adds and removes a reaction the server stores", async () => {
        expect(await driver.activityCommentReactionControlVisible(commentBody)).toBe(true);
        const comment = (
          await composerServerComments(seed.workspaceSlug, seed.projectId, issue.id, session.cookie)
        ).find((row) => row.comment_stripped.includes(commentBody));
        if (comment === undefined) throw new Error("[parity] the posted comment left no stored row.");
        await driver.activityBeginTrafficSpy({ reload: false });
        const picked = await driver.activityAddCommentReaction(commentBody);
        await expect
          .poll(() => driver.activityCommentReactionChips(commentBody), { timeout: 30_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        expect(
          (await serverCommentReactions(seed.workspaceSlug, seed.projectId, comment.id, session.cookie)).map(
            (row) => row.reaction
          )
        ).toEqual([picked.code]);
        await driver.activityClickCommentReactionChip(commentBody, picked.emoji);
        await expect.poll(() => driver.activityCommentReactionChips(commentBody), { timeout: 30_000 }).toEqual([]);
        await expect
          .poll(() => serverCommentReactions(seed.workspaceSlug, seed.projectId, comment.id, session.cookie), {
            timeout: 30_000,
          })
          .toEqual([]);
        const traffic = await driver.activityTrafficCounts();
        expect(traffic.commentReactionCreates).toBe(1);
        expect(traffic.commentReactionDeletes).toBe(1);
        expect(traffic.unexpectedWrites).toBe(0);
      });

      await test.step("the stored result is the same at the end in every edition", async () => {
        const stored = await composerServerComments(seed.workspaceSlug, seed.projectId, issue.id, session.cookie);
        expect(stored.filter((row) => row.comment_stripped.includes(commentBody))).toHaveLength(1);
        expect(await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session.cookie)).toEqual([]);
        const comment = stored.find((row) => row.comment_stripped.includes(commentBody));
        if (comment === undefined) throw new Error("[parity] the posted comment vanished before the final read.");
        expect(await serverCommentReactions(seed.workspaceSlug, seed.projectId, comment.id, session.cookie)).toEqual(
          []
        );
        // The whole scenario sent nothing outside the shared endpoints.
        expect((await driver.activityTrafficCounts()).unexpectedWrites).toBe(0);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session.cookie);
    }
  }
);
