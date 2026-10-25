// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: sub-work-items widget (NEWFRONT-121, Part C). The
// widget action row, collapsibles with content, progress rollup, creating
// and adding sub-issues, remove vs delete — against scenario-owned
// issues, green on apps/web first.
//
// Dev-only bug NEWFRONT-139 — on the dev oracle the widget panel never
// renders its rows (StrictMode double-toggle of the visibility flag). The
// production oracle renders rows with identifier and name, and every
// row-dependent assertion below polls for that. Row menus (remove/delete/
// edit) and inline property edits stay uncovered; see ISS-176/ISS-177.
// Rows: ISS-173, ISS-174, ISS-175, ISS-176.
import { test, expect } from "../fixtures";
import { createState, deleteState, fetchIssue, issueFacts, patchIssue, signInSession, subIssues } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-173"], "widget action row and sections render with content"),
  { tag: specTags(["ISS-173"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle widgets ${Date.now()}`);
    const child = await ownIssue(seed, session, `Oracle widget child ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("the action row offers every add control plus Run AI", async () => {
        for (const name of ["Add sub-work item", "Add relation", "Add link", "Attach", "Manually Run AI"]) {
          expect(await driver.page.getByRole("button", { name }).count(), name).toBeGreaterThan(0);
        }
      });
      await test.step("a fresh issue renders no widget sections", async () => {
        expect(await driver.widgetTitles()).toEqual([]);
      });
      await test.step("a sub-issue renders its section with a count", async () => {
        await patchIssue(seed.workspaceSlug, seed.projectId, child.id, session, { parent_id: issue.id });
        await driver.page.reload();
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
        await expect.poll(() => driver.widgetTitles(), { timeout: 30_000 }).toContain("Sub-work items");
        expect(await driver.widgetTitles()).not.toContain("Relations");
        expect(await driver.widgetTitles()).not.toContain("Links");
        expect(await driver.widgetTitles()).not.toContain("Attachments");
        expect(await driver.widgetProgress("Sub-work items")).toBe("0/1 Done");
      });
      await test.step("the panel body renders the child row", async () => {
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
          .toContain(child.name);
        const subs = await subIssues(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(subs.map((row) => row["name"])).toContain(child.name);
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-174"], "sub-issue progress rollup with rendered rows"),
  { tag: specTags(["ISS-174"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle rollup parent ${ts}`);
    const open = await ownIssue(seed, session, `Oracle rollup open ${ts}`);
    const done = await ownIssue(seed, session, `Oracle rollup done ${ts}`);
    const doneState = await createState(seed.workspaceSlug, seed.projectId, session, `Oracle done ${ts}`, "completed");
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, open.id, session, { parent_id: parent.id });
      await patchIssue(seed.workspaceSlug, seed.projectId, done.id, session, {
        parent_id: parent.id,
        state_id: doneState.id,
      });
      await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
      await driver.openWidgetSection("Sub-work items");
      await test.step("progress counts the completed child", async () => {
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("1/2 Done");
      });
      await test.step("both children render rows", async () => {
        for (const name of [open.name, done.name]) {
          await expect
            .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
            .toContain(name);
        }
        const subs = await subIssues(seed.workspaceSlug, seed.projectId, parent.id, session);
        expect(subs.map((row) => row["name"]).sort()).toEqual([done.name, open.name].sort());
      });
    } finally {
      await dropIssue(seed, session, done.id);
      await dropIssue(seed, session, open.id);
      await dropIssue(seed, session, parent.id);
      await deleteState(seed.workspaceSlug, seed.projectId, doneState.id, session).catch(() => {});
    }
  }
);

test(
  specTitle(["ISS-175"], "sub-issue create-new modal and add-existing"),
  { tag: specTags(["ISS-175"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle subadd parent ${ts}`);
    const existing = await ownIssue(seed, session, `Oracle subadd existing ${ts}`);
    const createdName = `Oracle subadd created ${ts}`;
    try {
      await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("create-new presets the parent and locks the project", async () => {
        await driver.openSubIssueCreateModal();
        expect(await driver.createModalParentName()).toContain(parent.name);
        expect(await driver.createModalProjectLocked()).toBe(true);
        await driver.createModalSubmit(createdName);
      });
      await test.step("the created child links to the parent and bumps the count", async () => {
        await expect
          .poll(async () => (await subIssues(seed.workspaceSlug, seed.projectId, parent.id, session)).length, {
            timeout: 60_000,
          })
          .toBe(1);
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 60_000 }).toBe("0/1 Done");
      });
      await test.step("add-existing links a second child", async () => {
        await driver.addExistingSubIssue(existing.name, existing.name);
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, existing.id, session))["parent_id"], {
            timeout: 30_000,
          })
          .toBe(parent.id);
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 60_000 }).toBe("0/2 Done");
      });
      await test.step("both children render rows", async () => {
        for (const name of [createdName, existing.name]) {
          await expect
            .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
            .toContain(name);
        }
      });
    } finally {
      const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session).catch(() => []);
      const created = rows.find((row) => row.name === createdName);
      if (created) await dropIssue(seed, session, created.id);
      await dropIssue(seed, session, existing.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);

test(
  specTitle(["ISS-176"], "sub-issue remove vs delete with rendered rows"),
  { tag: specTags(["ISS-176"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle subdel parent ${ts}`);
    const detached = await ownIssue(seed, session, `Oracle subdel detach ${ts}`);
    const destroyed = await ownIssue(seed, session, `Oracle subdel destroy ${ts}`);
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, detached.id, session, { parent_id: parent.id });
      await patchIssue(seed.workspaceSlug, seed.projectId, destroyed.id, session, { parent_id: parent.id });
      await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
      await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("0/2 Done");
      await test.step("both children render rows", async () => {
        // The per-row ellipsis menus stay uncovered: detach/delete below go
        // through the API and assert the header counts, not the menus.
        for (const name of [detached.name, destroyed.name]) {
          await expect
            .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
            .toContain(name);
        }
      });
      await test.step("remove (detach) keeps the issue and drops the count", async () => {
        await patchIssue(seed.workspaceSlug, seed.projectId, detached.id, session, { parent_id: null });
        expect((await fetchIssue(seed.workspaceSlug, seed.projectId, detached.id, session))["parent_id"]).toBeNull();
        await driver.page.reload();
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 60_000 }).toBe("0/1 Done");
      });
      await test.step("delete destroys the issue and drops the count", async () => {
        await dropIssue(seed, session, destroyed.id);
        const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session);
        expect(rows.map((row) => row.name)).not.toContain(destroyed.name);
        await driver.page.reload();
        await expect.poll(() => driver.widgetTitles(), { timeout: 60_000 }).not.toContain("Sub-work items");
      });
    } finally {
      await dropIssue(seed, session, destroyed.id).catch(() => {});
      await dropIssue(seed, session, detached.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);

/** Synthetic child rows for the stubbed paged read (ISS-238). */
function stubChildRows(
  parentId: string,
  projectId: string,
  count: number,
  sequenceStart: number,
  subIssuesCount = 0
): Record<string, unknown>[] {
  const stamp = "2026-10-01T10:00:00Z";
  return Array.from({ length: count }, (_, index) => ({
    id: crypto.randomUUID(),
    name: `Paged child ${sequenceStart + index}`,
    state_id: null,
    sort_order: sequenceStart + index,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: sequenceStart + index,
    project_id: projectId,
    parent_id: parentId,
    created_at: stamp,
    updated_at: stamp,
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    cycle_id: null,
    link_count: 0,
    attachment_count: 0,
    sub_issues_count: subIssuesCount,
    label_ids: [],
    assignee_ids: [],
    module_ids: [],
    state_group: "unstarted",
  }));
}

test(
  specTitle(["ISS-238"], "sub-issue pagination with a per-parent Load more"),
  { tag: specTags(["ISS-238"]) },
  async ({ driver, seed }, testInfo) => {
    // New-app-first: the OSS oracle reads every child in one unpaged
    // answer, so the paged widget only exists on apps/web_new.
    test.skip(testInfo.project.name !== "new", "sub-issue pagination pins the web_new UI only");
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Paged parent ${ts}`);
    const smallParent = await ownIssue(seed, session, `Unpaged parent ${ts}`);
    const smallChild = await ownIssue(seed, session, `Unpaged child ${ts}`);
    const viewed: string[] = [];
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, smallChild.id, session, { parent_id: smallParent.id });
      await test.step("a small parent loads in one request with no control", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, smallParent.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(smallParent.name);
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("0/1 Done");
        await driver.openWidgetSection("Sub-work items");
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).join("\n"), { timeout: 30_000 })
          .toContain(smallChild.name);
        expect(await driver.widgetLoadMorePresent("Sub-work items")).toBe(false);
      });
      const firstPage = stubChildRows(parent.id, seed.projectId, 50, 1000);
      const secondPage = stubChildRows(parent.id, seed.projectId, 4, 2000);
      // The first row is itself a parent, so its own cursor is separate
      // from the section's.
      const nestedParent = firstPage[0] as Record<string, unknown>;
      nestedParent["sub_issues_count"] = 1;
      const nestedName = String(nestedParent["name"]);
      const nestedFirstPage = stubChildRows(String(nestedParent["id"]), seed.projectId, 2, 3000);
      const nestedSecondPage = stubChildRows(String(nestedParent["id"]), seed.projectId, 2, 3002);
      // The full set is 120 children with one completed; the pages carry
      // the same full-set rollup (target per main#496).
      const distribution = { completed: [crypto.randomUUID()], unstarted: firstPage.map((row) => row["id"]) };
      await test.step("the first page stops at fifty rows and offers Load more", async () => {
        await driver.page.route(
          `**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${parent.id}/sub-issues/**`,
          async (route) => {
            const url = new URL(route.request().url());
            const cursor = url.searchParams.get("cursor");
            viewed.push(url.search);
            // The second page answers late so the widget's fetching state
            // has a moment to render before the rows land.
            if (cursor === "page-2") await new Promise((resolve) => setTimeout(resolve, 750));
            await route.fulfill({
              status: 200,
              contentType: "application/json",
              body: JSON.stringify({
                sub_issues: cursor === "page-2" ? secondPage : firstPage,
                state_distribution: distribution,
                total_count: 120,
                ...(cursor === "page-2"
                  ? { next_cursor: "", next_page_results: false }
                  : { next_cursor: "page-2", next_page_results: true }),
              }),
            });
          }
        );
        await driver.openIssueDetail(seed.workspaceSlug, parent.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(parent.name);
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("1/120 Done");
        await driver.openWidgetSection("Sub-work items");
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).length, { timeout: 30_000 })
          .toBe(50);
        expect(await driver.widgetLoadMorePresent("Sub-work items")).toBe(true);
        expect(viewed[0]).toContain("per_page=50");
        expect(viewed[0]).not.toContain("cursor=");
      });
      await test.step("Load more shows its fetching state, then appends the next cursor page", async () => {
        // The control disables and swaps in its spinner while the next
        // page is in flight, so the read has to land during the request.
        const loading = driver.widgetLoadMore("Sub-work items");
        try {
          await expect.poll(() => driver.widgetLoadMoreFetching("Sub-work items"), { timeout: 5_000 }).toBe(true);
        } finally {
          await loading;
        }
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).length, { timeout: 30_000 })
          .toBe(54);
        const names = (await driver.widgetRowNames("Sub-work items")).join("\n");
        expect(names).toContain("Paged child 1000");
        expect(names).toContain("Paged child 2003");
        expect(viewed.some((search) => search.includes("cursor=page-2"))).toBe(true);
      });
      await test.step("a nested parent keeps its own cursor page", async () => {
        // The row's own read is a separate collection: it starts at the
        // same fifty-row cap with no cursor, appends only its own next
        // page, and drops its control when the last one lands.
        const nestedReads: string[] = [];
        await driver.page.route(
          `**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${String(nestedParent["id"])}/sub-issues/**`,
          async (route) => {
            const url = new URL(route.request().url());
            const cursor = url.searchParams.get("cursor");
            nestedReads.push(url.search);
            await route.fulfill({
              status: 200,
              contentType: "application/json",
              body: JSON.stringify({
                sub_issues: cursor === "nested-2" ? nestedSecondPage : nestedFirstPage,
                state_distribution: {
                  completed: [],
                  unstarted: [...nestedFirstPage, ...nestedSecondPage].map((row) => row["id"]),
                },
                total_count: 4,
                ...(cursor === "nested-2"
                  ? { next_cursor: "", next_page_results: false }
                  : { next_cursor: "nested-2", next_page_results: true }),
              }),
            });
          }
        );
        await driver.expandWidgetRowChildren("Sub-work items", nestedName);
        expect(nestedReads[0]).toContain("per_page=50");
        expect(nestedReads[0]).not.toContain("cursor=");
        expect(await driver.widgetRowLoadMorePresent("Sub-work items", nestedName)).toBe(true);
        const before = (await driver.widgetRowNames("Sub-work items")).length;
        await driver.widgetRowLoadMore("Sub-work items", nestedName);
        await expect
          .poll(async () => (await driver.widgetRowNames("Sub-work items")).length, { timeout: 30_000 })
          .toBe(before + 2);
        const names = (await driver.widgetRowNames("Sub-work items")).join("\n");
        for (const child of ["Paged child 3000", "Paged child 3001", "Paged child 3002", "Paged child 3003"]) {
          expect(names).toContain(child);
        }
        // The section's own rows did not change: only the nested cursor moved.
        expect(names).toContain("Paged child 2003");
        expect(nestedReads.some((search) => search.includes("cursor=nested-2"))).toBe(true);
        await expect
          .poll(() => driver.widgetRowLoadMorePresent("Sub-work items", nestedName), { timeout: 30_000 })
          .toBe(false);
      });
      await test.step("the rollup keeps the full child set and the control hides", async () => {
        expect(await driver.widgetProgress("Sub-work items")).toBe("1/120 Done");
        await expect.poll(() => driver.widgetLoadMorePresent("Sub-work items"), { timeout: 30_000 }).toBe(false);
      });
    } finally {
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      await dropIssue(seed, session, smallChild.id);
      await dropIssue(seed, session, smallParent.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);
