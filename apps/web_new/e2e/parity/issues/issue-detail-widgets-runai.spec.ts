// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: Run AI and Comment & Run (NEWFRONT-121, Part C). The
// ISS-192 scenario pins its issue to a scenario-owned empty pod, so
// dispatch deterministically reports failure (no runner in that pod can
// serve it); the seeded default pod enrolls runners, so an unpinned
// issue would dispatch successfully instead. The ISS-193 continuation
// needs no pin: a fresh issue has no prior run, so its dispatch fails
// before runner eligibility is ever consulted. Against scenario-owned
// issues, green on apps/web first.
// Rows: ISS-192, ISS-193.
import { test, expect } from "../fixtures";
import { createPod, deletePod, fetchIssue, issueComments, patchIssue, recentRuns, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-192"], "manually run AI reports dispatch failure without runners"),
  { tag: specTags(["ISS-192"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    // The seeded default pod enrolls runners, so an unpinned issue would
    // dispatch successfully; pin the scenario-owned issue to a
    // scenario-owned empty pod instead, which has no runners by
    // construction and restores the deterministic graceful-failure premise.
    const emptyPod = await createPod(seed.projectId, session, `orunai-empty-${Date.now()}`);
    const issue = await ownIssue(seed, session, `Oracle runai ${Date.now()}`);
    try {
      // The PATCH response is narrow, so the pin is read back with a GET.
      await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, {
        assigned_pod_id: emptyPod.id,
      });
      const pin = (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["assigned_pod_id"];
      expect(pin).toBe(emptyPod.id);
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("the action row offers Manually Run AI", async () => {
        expect(await driver.page.getByRole("button", { name: "Manually Run AI" }).count()).toBeGreaterThan(0);
      });
      await test.step("dispatch fails gracefully and creates no run", async () => {
        await driver.clickWidgetAction("Manually Run AI");
        const failed = driver.target === "web_new" ? /couldn't start the run/i : /failed to start agent run/i;
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(failed);
        const runs = await recentRuns(session);
        expect(runs.filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
      });
    } finally {
      try {
        await dropIssue(seed, session, issue.id);
      } finally {
        await deletePod(emptyPod.id, session).catch(() => undefined);
      }
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
          .toMatch(
            driver.target === "web_new" ? /1::[\s\S]*couldn't start the run/i : /1::[\s\S]*failed to start agent run/i
          );
        expect((await recentRuns(session)).filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
