// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: Run AI and Comment & Run (NEWFRONT-121, Part C). The
// seeded stack enrolls no runners, so dispatch deterministically reports
// failure; these scenarios pin that graceful-failure path plus the
// comment-first ordering. Against scenario-owned issues, green on
// apps/web first.
// Rows: ISS-192, ISS-193.
import { test, expect } from "../fixtures";
import { issueComments, recentRuns, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-192"], "manually run AI reports dispatch failure without runners"),
  { tag: specTags(["ISS-192"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle runai ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("the action row offers Manually Run AI", async () => {
        expect(await driver.page.getByRole("button", { name: "Manually Run AI" }).count()).toBeGreaterThan(0);
      });
      await test.step("dispatch fails gracefully and creates no run", async () => {
        await driver.clickWidgetAction("Manually Run AI");
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/failed to start agent run/i);
        const runs = await recentRuns(session);
        expect(runs.filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-193"], "comment and run posts first, then dispatches; empty short-circuits"),
  { tag: specTags(["ISS-193"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle commentrun ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("empty composer disables the action: no comment and no run", async () => {
        await expect.poll(() => driver.commentAndRunDisabled(), { timeout: 30_000 }).toBe(true);
        expect(await issueComments(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
        expect((await recentRuns(session)).filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
      });
      await test.step("a typed comment posts, then dispatch is attempted", async () => {
        const body = `Oracle run prompt ${ts}`;
        await driver.typeComment(body);
        await expect.poll(() => driver.commentAndRunDisabled(), { timeout: 30_000 }).toBe(false);
        await driver.clickCommentAndRun();
        // One poll for both halves: the dispatch round-trip is slow on a
        // contended stack, and separate polls would burn the budget twice.
        await expect
          .poll(
            async () => {
              const count = (await issueComments(seed.workspaceSlug, seed.projectId, issue.id, session)).length;
              const toast = (await driver.lastToast()) ?? "";
              return `${count}::${toast}`;
            },
            { timeout: 120_000 }
          )
          .toMatch(/1::[\s\S]*failed to start agent run/i);
        expect((await recentRuns(session)).filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
