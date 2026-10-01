// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: relations widget (NEWFRONT-121, Part C). Grouped
// relation lists, adding through the type menu, reciprocal edges, row
// actions, and the four relation types — against scenario-owned issues,
// green on apps/web first.
// Rows: ISS-179, ISS-180, ISS-181, ISS-182, ISS-183.
import { test, expect } from "../fixtures";
import { addRelation, issueFacts, issueRelations, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-179", "ISS-183"], "relations group by type with the four known kinds"),
  { tag: specTags(["ISS-179", "ISS-183"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle rel view ${ts}`);
    const blocked = await ownIssue(seed, session, `Oracle rel blocked ${ts}`);
    const related = await ownIssue(seed, session, `Oracle rel related ${ts}`);
    try {
      await addRelation(seed.workspaceSlug, seed.projectId, issue.id, session, {
        relation_type: "blocked_by",
        issues: [blocked.id],
      });
      await addRelation(seed.workspaceSlug, seed.projectId, issue.id, session, {
        relation_type: "relates_to",
        issues: [related.id],
      });
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await driver.openWidgetSection("Relations");
      await test.step("groups render per present type with their rows", async () => {
        await expect
          .poll(async () => (await driver.widgetGroupNames("Relations")).join(" | "), { timeout: 30_000 })
          .toContain("Blocked");
        const groups = await driver.widgetGroupNames("Relations");
        expect(groups.join(" | ")).toContain("Relates");
        const rows = await driver.widgetRowNames("Relations");
        expect(rows.join(" | ")).toContain(blocked.name);
        expect(rows.join(" | ")).toContain(related.name);
        expect(rows.join(" | ")).toContain(blocked.seq);
      });
      await test.step("the add menu names the four relation types", async () => {
        const names = await driver.widgetAddMenuNames("Relations");
        for (const kind of ["Blocked", "Blocking", "Duplicate", "Relates"]) {
          expect(
            names.some((entry) => entry.includes(kind)),
            kind
          ).toBe(true);
        }
        await driver.page.keyboard.press("Escape");
      });
      await test.step("the server agrees", async () => {
        const rels = await issueRelations(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(rels.length).toBe(2);
      });
    } finally {
      await dropIssue(seed, session, related.id);
      await dropIssue(seed, session, blocked.id);
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-180"], "add a relation through the type menu"),
  { tag: specTags(["ISS-180"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle rel add ${ts}`);
    const other = await ownIssue(seed, session, `Oracle rel other ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await driver.addRelationViaModal("Blocking", other.name, other.name);
      await test.step("the row renders under its group", async () => {
        await driver.openWidgetSection("Relations");
        await expect
          .poll(async () => (await driver.widgetRowNames("Relations")).join(" | "), { timeout: 60_000 })
          .toContain(other.name);
      });
      await test.step("the server agrees", async () => {
        await expect
          .poll(async () => (await issueRelations(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
            timeout: 30_000,
          })
          .toBe(1);
      });
    } finally {
      await dropIssue(seed, session, other.id);
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-181"], "relation edges stay reciprocal"),
  { tag: specTags(["ISS-181"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle rel recip ${ts}`);
    const other = await ownIssue(seed, session, `Oracle rel peer ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("adding blocked-by records blocking on the peer", async () => {
        await driver.addRelationViaModal("Blocked", other.name, other.name);
        await expect
          .poll(
            async () =>
              (await issueRelations(seed.workspaceSlug, seed.projectId, other.id, session)).map(
                (row) => row["relation_group"]
              ),
            { timeout: 30_000 }
          )
          .toContain("blocking");
      });
      await test.step("the peer shows the inverse row", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, other.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(other.name);
        await driver.openWidgetSection("Relations");
        await expect
          .poll(async () => (await driver.widgetRowNames("Relations")).join(" | "), { timeout: 30_000 })
          .toContain(issue.name);
      });
      await test.step("removing from one side clears both", async () => {
        await driver.clickWidgetRowAction("Relations", issue.name, "Remove");
        await expect
          .poll(async () => (await issueRelations(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
            timeout: 30_000,
          })
          .toBe(0);
        expect(await issueRelations(seed.workspaceSlug, seed.projectId, other.id, session)).toHaveLength(0);
      });
    } finally {
      await dropIssue(seed, session, other.id);
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-182"], "relation row actions remove and delete"),
  { tag: specTags(["ISS-182"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle rel acts ${ts}`);
    const detached = await ownIssue(seed, session, `Oracle rel detach ${ts}`);
    const destroyed = await ownIssue(seed, session, `Oracle rel destroy ${ts}`);
    try {
      await addRelation(seed.workspaceSlug, seed.projectId, issue.id, session, {
        relation_type: "relates_to",
        issues: [detached.id, destroyed.id],
      });
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await driver.openWidgetSection("Relations");
      await expect
        .poll(async () => (await driver.widgetRowNames("Relations")).join(" | "), { timeout: 30_000 })
        .toContain(detached.name);
      await test.step("the row menu offers remove, edit, delete, and copy link", async () => {
        const names = await driver.widgetRowActionNames("Relations", detached.name);
        for (const action of ["Remove", "Edit", "Delete", "Copy link"]) {
          expect(
            names.some((entry) => entry.includes(action)),
            action
          ).toBe(true);
        }
        await driver.page.keyboard.press("Escape");
      });
      await test.step("remove detaches both ways and keeps the issue", async () => {
        await driver.clickWidgetRowAction("Relations", detached.name, "Remove");
        await expect
          .poll(async () => (await issueRelations(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
            timeout: 30_000,
          })
          .toBe(1);
        expect(await issueRelations(seed.workspaceSlug, seed.projectId, detached.id, session)).toHaveLength(0);
        const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session);
        expect(rows.map((row) => row.name)).toContain(detached.name);
      });
      await test.step("delete unrelates then destroys behind a confirm", async () => {
        await driver.clickWidgetRowAction("Relations", destroyed.name, "Delete");
        await expect.poll(() => driver.confirmModalTitle(), { timeout: 15_000 }).not.toBeNull();
        await driver.confirmModal("Delete");
        await expect
          .poll(async () => (await issueRelations(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
            timeout: 30_000,
          })
          .toBe(0);
        const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session);
        expect(rows.map((row) => row.name)).not.toContain(destroyed.name);
      });
    } finally {
      await dropIssue(seed, session, destroyed.id).catch(() => {});
      await dropIssue(seed, session, detached.id);
      await dropIssue(seed, session, issue.id);
    }
  }
);
