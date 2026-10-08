// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): GitHub code-review links on an issue.
// Row: ISS-205 (view / attach / detach).
import { test, expect } from "../fixtures";
import {
  serverAttachCodeReview,
  serverCodeReviews,
  serverCreateIssueFull,
  serverCleanupIssueWithSession,
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

// Review links are globally unique per repository, and sibling runs share
// this stack, so each test mints its own repository name. PR numbers stay
// small: the legacy link table stores them in an int32 column.
const reviewRepo = (tag: string): string => `hello-world-nf122-${tag.replace(/[^0-9]/g, "").slice(-10)}`;
const reviewUrl = (repo: string, prNumber: number): string => `https://github.com/octocat/${repo}/pull/${prNumber}`;

test(
  specTitle(["ISS-205"], "code review links attach, display and detach"),
  { tag: specTags(["ISS-205"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 reviews ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const repo = reviewRepo(tag);
      const firstUrl = reviewUrl(repo, 1);
      const secondUrl = reviewUrl(repo, 2);
      await openOwnIssue(driver, seed, issue.id);

      await test.step("section stays hidden until a review is linked", async () => {
        expect(await driver.codeReviewsVisible()).toBe(false);
        expect(await serverCodeReviews(seed.workspaceSlug, seed.projectId, issue.id, session)).toEqual([]);
      });

      await test.step("a linked review renders badge, title and a new-tab link", async () => {
        await serverAttachCodeReview(seed.workspaceSlug, seed.projectId, issue.id, firstUrl, session);
        await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        await expect.poll(() => driver.codeReviewsVisible(), { timeout: 30_000 }).toBe(true);
        const links = await driver.codeReviewLinks();
        expect(links).toHaveLength(1);
        expect(links[0]!.href).toBe(firstUrl);
        expect(links[0]!.target).toBe("_blank");
        expect(links[0]!.badge.length).toBeGreaterThan(0);
        expect(links[0]!.title).toContain(`octocat/${repo}`);
      });

      await test.step("attaching through the form clears the input and adds a row", async () => {
        await driver.codeReviewAttach(secondUrl);
        await expect.poll(() => driver.codeReviewLinks(), { timeout: 30_000 }).toHaveLength(2);
        const server = await serverCodeReviews(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(server.map((r) => r.url).sort()).toEqual([firstUrl, secondUrl].sort());
      });

      await test.step("detaching removes the row and hides the section when empty", async () => {
        const before = await driver.codeReviewLinks();
        const second = before.find((l) => l.href === secondUrl);
        expect(second).toBeDefined();
        await driver.codeReviewDetach(second!.title);
        await expect.poll(() => driver.codeReviewLinks(), { timeout: 30_000 }).toHaveLength(1);
        const first = (await driver.codeReviewLinks())[0]!;
        await driver.codeReviewDetach(first.title);
        await expect.poll(() => driver.codeReviewsVisible(), { timeout: 30_000 }).toBe(false);
        expect(await serverCodeReviews(seed.workspaceSlug, seed.projectId, issue.id, session)).toEqual([]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-205"], "attaching an invalid review URL surfaces an error"),
  { tag: specTags(["ISS-205"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 badreview ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      // The attach form lives inside the section, so bootstrap one link first.
      await serverAttachCodeReview(
        seed.workspaceSlug,
        seed.projectId,
        issue.id,
        reviewUrl(reviewRepo(tag), 1),
        session
      );
      await openOwnIssue(driver, seed, issue.id);
      await expect.poll(() => driver.codeReviewsVisible(), { timeout: 30_000 }).toBe(true);

      await test.step("server errors toast while the input keeps its value", async () => {
        // A syntactically valid URL that no adapter parses: native url
        // validation lets the submit through and the server 400 surfaces.
        const badUrl = "https://example.com/not-a-review";
        await driver.codeReviewAttemptAttach(badUrl);
        await expect.poll(() => driver.sawToast("Code review not attached"), { timeout: 30_000 }).toBe(true);
        expect(await driver.codeReviewInputValue()).toBe(badUrl);
        const server = await serverCodeReviews(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(server).toHaveLength(1);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
