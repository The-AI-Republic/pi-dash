// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: sub-work-item rows and filters (NEWFRONT-121, Part C).
// bug: NEWFRONT-139 — the widget panel never renders rows on the seeded
// dev stack, so the inline edits, full edit modal, row click, and every
// list-dependent filter assertion below are unreachable. These scenarios
// pin the observed gap plus the reachable header controls; intended
// behavior is recorded in each row.
// Rows: ISS-177, ISS-178.
import { test, expect } from "../fixtures";
import { patchIssue, signInSession, subIssues } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-177"], "bug: NEWFRONT-139 sub-issue row edits need rendered rows"),
  { tag: specTags(["ISS-177"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle rowedit parent ${ts}`);
    const child = await ownIssue(seed, session, `Oracle rowedit child ${ts}`);
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, child.id, session, { parent_id: parent.id });
      await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
      await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("0/1 Done");
      await test.step("bug: no rows render for inline edits, the edit modal, or row click", async () => {
        // Intended: rows show inline State/Priority/date/Assignee controls
        // (each patching one field), an "Edit work item" menu action opens
        // the full modal, and clicking a row opens the child in peek.
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
        const subs = await subIssues(seed.workspaceSlug, seed.projectId, parent.id, session);
        expect(subs.map((row) => row["name"])).toContain(child.name);
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);

test(
  specTitle(["ISS-178"], "bug: NEWFRONT-139 sub-issue filters without a rendered list"),
  { tag: specTags(["ISS-178"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle rowfilter parent ${ts}`);
    const child = await ownIssue(seed, session, `Oracle rowfilter child ${ts}`);
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, child.id, session, { parent_id: parent.id });
      await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
      await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("0/1 Done");
      await test.step("the header offers filter and display controls", async () => {
        expect(await driver.widgetHeaderControlCount("Sub-work items")).toBeGreaterThan(0);
      });
      await test.step("bug: filtering has no visible list to act on", async () => {
        // Intended: the Filters dropdown (priority, state group, state,
        // project, type, assignees, start/due) plus display-filters
        // (group-by, order-by, display properties) filter/group the rows;
        // an empty filtered result shows "Clear filters". Unobservable
        // while rows don't render; filters are client-side per parent.
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
        const subs = await subIssues(seed.workspaceSlug, seed.projectId, parent.id, session);
        expect(subs.map((row) => row["name"])).toContain(child.name);
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);
