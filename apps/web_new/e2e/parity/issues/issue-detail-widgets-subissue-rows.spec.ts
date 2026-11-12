// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: sub-work-item rows and filters (NEWFRONT-121, Part C).
// Dev-only bug NEWFRONT-139 — the widget panel never renders rows on the
// dev oracle (StrictMode double-toggle of the visibility flag). The
// production oracle renders the rows, and both scenarios below poll for
// that plus the reachable header controls. Inline edits, the full edit
// modal, row click, and list-dependent filter behavior stay uncovered.
// Rows: ISS-177, ISS-178.
import { test, expect } from "../fixtures";
import { patchIssue, signInSession, subIssues } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-177"], "sub-issue rows render for edits"),
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
      await test.step("the child renders a row for edits", async () => {
        // Covered: the row renders with identifier and name. Still open:
        // inline State/Priority/date/Assignee controls (each patching one
        // field), the "Edit work item" menu action opening the full modal,
        // and clicking a row opening the child in peek.
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
          .toContain(child.name);
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
  specTitle(["ISS-178"], "sub-issue filters with a rendered list"),
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
      await test.step("filtering has a visible list to act on", async () => {
        // Covered: the row renders beneath the filter controls. Still
        // open: the Filters dropdown (priority, state group, state,
        // project, type, assignees, start/due) plus display-filters
        // (group-by, order-by, display properties) filtering/grouping the
        // rows, and the "Clear filters" empty state. Filters are
        // client-side per parent.
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
          .toContain(child.name);
        const subs = await subIssues(seed.workspaceSlug, seed.projectId, parent.id, session);
        expect(subs.map((row) => row["name"])).toContain(child.name);
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);
