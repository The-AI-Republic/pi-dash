// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): de-dupe finds near-duplicate work items
// in the create modal and on the issue detail. Rows: ISS-121 (modal),
// ISS-231 (detail popover). Edition-gated proof per
// parity/specs/issues.md §23: the OSS legs live in create-modal.spec.ts
// and edition-stubs.spec.ts, this file proves the cloud replacement. It
// runs on the "new" project only — the OSS oracle provably lacks the
// surface. Auth rides API session cookies (the cloud /login is OIDC-only).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-121"], "cloud: typing a duplicate title offers the match in the modal"),
  { tag: specTags(["ISS-121"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud de-dupe pins the web_new UI only");
    const tag = `NF984 dedupe ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    // One distinguishing word per title: the near-duplicate rule needs
    // more than half the shorter title's words shared.
    const issueName = `Harbor pylon calibration audit${Date.now()}`;
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, issueName, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();

      await test.step("typing the duplicate title raises the count button", async () => {
        await driver.openCreateModal();
        await driver.fillCreateTitle(issueName);
        await expect(page.getByRole("button", { name: "1 possible duplicate" })).toBeVisible({ timeout: 60_000 });
      });

      await test.step("opening the panel lists the match", async () => {
        await page.getByRole("button", { name: "1 possible duplicate" }).click();
        const panel = page.getByRole("complementary", { name: "Possible duplicates" });
        await expect(panel).toBeVisible();
        const match = panel.getByRole("link", { name: issueName });
        await expect(match).toBeVisible();
        expect(await match.getAttribute("href")).toContain("browse");
      });

      await test.step("discard the untouched draft", async () => {
        await driver.clickModalDiscard();
        expect(await driver.createModalOpen()).toBe(false);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-231"], "cloud: the detail shows a duplicate popover for near matches"),
  { tag: specTags(["ISS-231"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud de-dupe pins the web_new UI only");
    const tag = `NF984 dupop ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    // Folded-identical titles: the server search ANDs its lexemes, so a
    // near-dupe the server cannot retrieve can never match client-side.
    // The shared stamp keeps other runs' leftovers out of the result.
    const stamp = Date.now();
    const firstName = `Harbor pylon calibration audit ${stamp}`;
    const secondName = `Harbor pylon calibration audit! ${stamp}`;
    const first = await serverCreateIssueFull(seed.workspaceSlug, projectId, firstName, session);
    const second = await serverCreateIssueFull(seed.workspaceSlug, projectId, secondName, session);
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, second.id);

      await test.step("the near match raises the popover trigger", async () => {
        const popover = page.getByTestId("detail-duplicate-popover");
        await expect(popover).toBeVisible({ timeout: 60_000 });
        await expect(popover.getByRole("button", { name: "1 possible duplicate" })).toBeVisible();
      });

      await test.step("opening the popover lists the match", async () => {
        await page
          .getByTestId("detail-duplicate-popover")
          .getByRole("button", { name: "1 possible duplicate" })
          .click();
        const match = page.getByRole("link", { name: firstName });
        await expect(match).toBeVisible();
        expect(await match.getAttribute("href")).toContain("browse");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, first.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, second.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
