// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): issue activity feed — merged timeline,
// filter toggles and sort order with composer placement.
// Rows: ISS-194 (feed view), ISS-195 (feed filter), ISS-196 (sort & composer).
import { test, expect } from "../fixtures";
import {
  commentText,
  serverComments,
  serverCreateIssueFull,
  serverCleanupIssueWithSession,
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
  specTitle(["ISS-194"], "activity feed merges creation entry and comments"),
  { tag: specTags(["ISS-194"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 feed ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await test.step("fresh issue shows only the creation entry", async () => {
        await openOwnIssue(driver, seed, issue.id);
        expect(await driver.activityHasCreationEntry()).toBe(true);
        expect(await driver.activityCommentTexts()).toEqual([]);
      });

      await test.step("a posted comment joins the timeline with actor and timestamp", async () => {
        await driver.activityPostComment(`${tag} hello`);
        const texts = await driver.activityCommentTexts();
        expect(texts).toHaveLength(1);
        expect(texts[0]).toContain(`${tag} hello`);
        expect(texts[0]).toContain("Parity Oracle");
        expect(texts[0]).toMatch(/ago|just now/i);
      });

      await test.step("the server stored the comment", async () => {
        const comments = await serverComments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(comments).toHaveLength(1);
        expect(commentText(comments[0]!.comment_html)).toContain(`${tag} hello`);
        expect(await driver.activityHasCreationEntry()).toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-195"], "activity feed filter toggles persist and guard the last option"),
  { tag: specTags(["ISS-195"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 filter ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${tag} comment</p>`, session);
      await openOwnIssue(driver, seed, issue.id);

      await test.step("four toggles start selected with no accent dot", async () => {
        const options = await driver.activityFilterOptions();
        expect(options.map((o) => o.label)).toEqual(["Updates", "Comments", "State", "Assignee"]);
        expect(options.every((o) => o.selected)).toBe(true);
        expect(await driver.activityFilterDotVisible()).toBe(false);
      });

      await test.step("hiding comments keeps the creation entry and marks the button", async () => {
        await driver.activityToggleFilter("Comments");
        await expect.poll(() => driver.activityCommentTexts(), { timeout: 15_000 }).toEqual([]);
        expect(await driver.activityHasCreationEntry()).toBe(true);
        expect(await driver.activityFilterDotVisible()).toBe(true);
      });

      await test.step("the filter survives reload", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        await expect.poll(() => driver.activityCommentTexts(), { timeout: 15_000 }).toEqual([]);
        const options = await driver.activityFilterOptions();
        expect(options.find((o) => o.label === "Comments")?.selected).toBe(false);
      });

      await test.step("re-enabling comments clears the dot", async () => {
        await driver.activityToggleFilter("Comments");
        await expect
          .poll(() => driver.activityCommentTexts(), { timeout: 15_000 })
          .toEqual([expect.stringContaining(`${tag} comment`)]);
        expect(await driver.activityFilterDotVisible()).toBe(false);
      });

      await test.step("deselecting the last option is a no-op", async () => {
        await driver.activityToggleFilter("Updates");
        await driver.activityToggleFilter("State");
        await driver.activityToggleFilter("Assignee");
        await driver.activityToggleFilter("Comments");
        const options = await driver.activityFilterOptions();
        expect(options.find((o) => o.label === "Comments")?.selected).toBe(true);
        await expect
          .poll(() => driver.activityCommentTexts(), { timeout: 15_000 })
          .toEqual([expect.stringContaining(`${tag} comment`)]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-196"], "feed sort flips order and moves the composer"),
  { tag: specTags(["ISS-196"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 sort ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${tag} first</p>`, session);
      await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${tag} second</p>`, session);
      await openOwnIssue(driver, seed, issue.id);

      const orderOf = async (): Promise<[number, number]> => {
        const texts = await driver.activityCommentTexts();
        return [
          texts.findIndex((t) => t.includes(`${tag} first`)),
          texts.findIndex((t) => t.includes(`${tag} second`)),
        ];
      };

      await test.step("ascending shows oldest first with the composer below", async () => {
        const [first, second] = await orderOf();
        expect(first).toBeGreaterThanOrEqual(0);
        expect(second).toBeGreaterThan(first);
        expect(await driver.activityComposerIsAboveFeed()).toBe(false);
      });

      await test.step("descending reverses and lifts the composer above", async () => {
        await driver.activityToggleSort();
        await expect
          .poll(async () => {
            const [first, second] = await orderOf();
            return first > second && first >= 0 && second >= 0;
          })
          .toBe(true);
        expect(await driver.activityComposerIsAboveFeed()).toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
