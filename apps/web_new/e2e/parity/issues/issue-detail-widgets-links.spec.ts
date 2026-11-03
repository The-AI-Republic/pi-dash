// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: links widget (NEWFRONT-121, Part C). Viewing links,
// add/edit through the shared modal, opening and removing — against
// scenario-owned issues, green on apps/web first.
// Rows: ISS-184, ISS-185, ISS-186.
import { test, expect } from "../fixtures";
import { addLink, issueLinks, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-184"], "view issue links with titles and attribution"),
  { tag: specTags(["ISS-184"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle links view ${ts}`);
    const bareUrl = `https://example.com/oracle-${ts}`;
    try {
      await addLink(seed.workspaceSlug, seed.projectId, issue.id, session, {
        url: "https://example.com/oracle-titled",
        title: `Oracle titled link ${ts}`,
      });
      await addLink(seed.workspaceSlug, seed.projectId, issue.id, session, { url: bareUrl });
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await driver.openWidgetSection("Links");
      await test.step("rows show the title or fall back to the URL", async () => {
        await expect
          .poll(async () => (await driver.widgetRowNames("Links")).join(" | "), { timeout: 30_000 })
          .toContain(`Oracle titled link ${ts}`);
        const rows = await driver.widgetRowNames("Links");
        expect(rows.join(" | ")).toContain(bareUrl);
        expect((await issueLinks(seed.workspaceSlug, seed.projectId, issue.id, session)).length).toBe(2);
      });
      await test.step("rows carry added-by attribution", async () => {
        const rows = await driver.widgetRowNames("Links");
        expect(rows.join(" | ")).toMatch(/minute ago|hour ago|just now|Added/);
      });
      await test.step("the copy control copies its URL", async () => {
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        await driver.clickLinkCopy(`Oracle titled link ${ts}`);
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/link copied/i);
        expect(await driver.readClipboard()).toContain("https://example.com/oracle-titled");
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-185"], "add and edit an external link through one modal"),
  { tag: specTags(["ISS-185"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle links edit ${ts}`);
    const title = `Oracle modal link ${ts}`;
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("add prefixes a missing scheme and stores the title", async () => {
        await driver.addLinkModal(`example.com/oracle-${ts}`, title);
        await driver.openWidgetSection("Links");
        await expect
          .poll(async () => (await driver.widgetRowNames("Links")).join(" | "), { timeout: 60_000 })
          .toContain(title);
        const links = await issueLinks(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(links).toHaveLength(1);
        expect(String(links[0]?.["url"] ?? "")).toMatch(/^https?:\/\//);
        expect(links[0]?.["title"]).toBe(title);
      });
      await test.step("edit reuses the modal for a new title", async () => {
        const renamed = `Oracle renamed link ${ts}`;
        await driver.editLinkTitle(title, renamed);
        await expect
          .poll(async () => (await driver.widgetRowNames("Links")).join(" | "), { timeout: 30_000 })
          .toContain(renamed);
        const links = await issueLinks(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(links[0]?.["title"]).toBe(renamed);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-186"], "open a link in a new tab and remove it"),
  { tag: specTags(["ISS-186"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle links open ${ts}`);
    const title = `Oracle open link ${ts}`;
    try {
      await addLink(seed.workspaceSlug, seed.projectId, issue.id, session, {
        url: "https://example.com/oracle-open",
        title,
      });
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await driver.openWidgetSection("Links");
      await expect
        .poll(async () => (await driver.widgetRowNames("Links")).join(" | "), { timeout: 30_000 })
        .toContain(title);
      await test.step("the row targets a new tab", async () => {
        const target = await driver.linkRowTarget(title);
        expect(target, "link target").not.toBeNull();
        expect(target!.href).toContain("https://example.com/oracle-open");
        expect(target!.target).toBe("_blank");
      });
      await test.step("remove deletes immediately with no confirm", async () => {
        await driver.clickWidgetRowAction("Links", title, "Delete");
        await expect.poll(() => driver.widgetRowNames("Links"), { timeout: 30_000 }).toHaveLength(0);
        expect(await driver.confirmModalTitle()).toBeNull();
        expect(await issueLinks(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
