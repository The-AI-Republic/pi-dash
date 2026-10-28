// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the functional bulk toolbar replaces the
// OSS upgrade banner, and the timeline offers selection checkboxes. Rows:
// ISS-113 (bulk delete), ISS-114 (bulk archive), ISS-116 (gantt bulk
// selection). Edition-gated proof per parity/specs/issues.md §23: the OSS
// legs live in bulk-selection.spec.ts, this file proves the cloud
// replacement. It runs on the "new" project only — the OSS oracle
// provably lacks the surface. Auth rides API session cookies (the cloud
// /login is OIDC-only). ISS-112 (update) and ISS-115 (subscribe) grow
// their legs once their endpoints land.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverIssue,
  serverPatchIssue,
  serverProjectStates,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-113"], "cloud: bulk delete removes the selection after confirmation"),
  { tag: specTags(["ISS-113"]) },
  async ({ driver, seed, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud bulk pins the web_new UI only");
    const tag = `NF984 bulkdel ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([`${tag} issue`]));

      await test.step("selecting a row raises the toolbar, not the banner", async () => {
        await driver.selectFirstRow();
        expect(await driver.bulkToolbarVisible()).toBe(true);
        expect(await driver.bulkBarVisible()).toBe(false);
      });

      await test.step("delete confirms and removes the issue", async () => {
        await driver.bulkToolbarDelete();
        await expect
          .poll(
            () =>
              serverIssue(seed.workspaceSlug, projectId, issue.id, session).then(
                () => "alive",
                () => "gone"
              ),
            { timeout: 30_000 }
          )
          .toBe("gone");
        expect(await driver.bulkToolbarVisible()).toBe(false);
      });

      await test.step("the screen no longer lists the issue", async () => {
        await expect
          .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
          .not.toEqual(expect.arrayContaining([`${tag} issue`]));
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-114"], "cloud: bulk archive stamps the selection as archived"),
  { tag: specTags(["ISS-114"]) },
  async ({ driver, seed, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud bulk pins the web_new UI only");
    const tag = `NF984 bulkarc ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // Archiving requires a completed or cancelled state; reuse a
    // project-default one when present, else mint a scenario-owned one.
    const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
    const seededDone = states.find((s) => s.group === "completed" || s.group === "cancelled");
    const doneStateId =
      seededDone?.id ?? (await serverCreateState(seed.workspaceSlug, projectId, `${tag} done`, "completed", session));
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: doneStateId }, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([`${tag} issue`]));

      await test.step("archive stamps the issue through the toolbar", async () => {
        await driver.selectFirstRow();
        expect(await driver.bulkToolbarVisible()).toBe(true);
        await driver.bulkToolbarArchive();
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).archived_at, {
            timeout: 30_000,
          })
          .not.toBeNull();
        expect(await driver.bulkToolbarVisible()).toBe(false);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-116"], "cloud: the timeline offers selection checkboxes and the toolbar"),
  { tag: specTags(["ISS-116"]) },
  async ({ driver, seed, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud bulk pins the web_new UI only");
    const tag = `NF984 ganttsel ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([`${tag} issue`]));

      await test.step("the timeline sidebar offers selection controls", async () => {
        await driver.ganttOpenTimeline();
        await expect.poll(() => driver.pageTextContains("Today"), { timeout: 120_000 }).toBe(true);
        // Rows stream in after the chrome: poll until the sidebar binds them.
        await expect.poll(() => driver.selectionCheckboxCount(), { timeout: 120_000 }).toBeGreaterThan(0);
        expect(await driver.bulkBarVisible()).toBe(false);
      });

      // No layout restore: every test gets a fresh browser context.
      await test.step("selecting a timeline row raises the toolbar", async () => {
        await driver.selectFirstRow();
        expect(await driver.bulkToolbarVisible()).toBe(true);
        expect(await driver.bulkBarVisible()).toBe(false);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
