// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: sub-work-items widget (NEWFRONT-121, Part C). The
// widget action row, collapsibles with content, progress rollup, creating
// and adding sub-issues, remove vs delete — against scenario-owned
// issues, green on apps/web first.
//
// bug: NEWFRONT-139 — on the seeded dev stack the widget panel never
// renders its rows (header counts are correct, the body stays empty), so
// every row-dependent assertion below documents the observed gap instead
// of the intended rows. Intended: rows render with identifier, name, and
// inline properties; row menus offer remove/delete/edit.
// Rows: ISS-173, ISS-174, ISS-175, ISS-176.
import { test, expect } from "../fixtures";
import { createState, deleteState, fetchIssue, issueFacts, patchIssue, signInSession, subIssues } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-173"], "bug: NEWFRONT-139 widget action row and sections render only with content"),
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
      await test.step("bug: the panel body stays empty though the server has the child", async () => {
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
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
  specTitle(["ISS-174"], "bug: NEWFRONT-139 sub-issue progress rollup without rendered rows"),
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
      await test.step("bug: rows stay unrendered though the server has both children", async () => {
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
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
  specTitle(["ISS-175"], "bug: NEWFRONT-139 sub-issue create-new modal and add-existing"),
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
      await test.step("bug: neither child renders a row", async () => {
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
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
  specTitle(["ISS-176"], "bug: NEWFRONT-139 sub-issue remove vs delete is unreachable without rows"),
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
      await test.step("bug: no rows render, so the row menus are unreachable", async () => {
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
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
