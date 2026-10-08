// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the saved-prompt preview form renders
// under an expanded receipt for admins only, takes an issue target for
// coding/review kinds and an install target for the scheduler kind, and
// refuses empty targets by disabling submit. BUG (NEWFRONT-193): submit
// 403s on CSRF, so the scenario locks in the inline CSRF failure and
// proves the intended render through the CSRF-paired API.
// Row: AGT-030.
import { test, expect } from "../../fixtures";
import {
  createIssue,
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverPromptPreview,
  serverPromptPreviewStatus,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";

const ROWS = ["AGT-030"];

test(
  specTitle(ROWS, "bug: NEWFRONT-193 saved preview 403s CSRF; admin gate, targets and the intended render hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt30"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const targets = await test.step("owner prepares an issue and an install", async () => {
      const project = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT30 Project ${tag}`,
        parityProjectIdentifier("AG30")
      );
      const issueName = `AGT30 preview issue ${tag}`;
      const issue = await createIssue(workspaceSlug, project.id, ownerSession, issueName);
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt30-def-${workspaceSlug}`,
        name: "AGT30 Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const binding = await ensureBinding(workspaceSlug, project.id, ownerSession, {
        scheduler: definition.id,
        project: project.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return { issueId: issue.id, issueName, bindingId: binding.id };
    });

    await test.step("owner signs in and expands the coding-task receipt", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsReceiptExpanded("coding-task")).toBe(true);
    });

    await test.step("the admin-only form refuses empty targets by disabling submit", async () => {
      expect(await driver.promptsSavedPreviewVisible("coding-task")).toBe(true);
      expect(await driver.promptsSavedPreviewSubmitEnabled("coding-task")).toBe(false);
    });

    await test.step("bug: the issue-targeted submit fails inline on CSRF", async () => {
      await driver.promptsSavedPreviewSubmit("coding-task", targets.issueId);
      await expect
        .poll(async () => (await driver.promptsSavedPreviewResult("coding-task")).error ?? "", { timeout: 30_000 })
        .not.toBe("");
      const result = await driver.promptsSavedPreviewResult("coding-task");
      expect(result.error).toContain("CSRF Failed");
      expect(result.prompt).toBeNull();
    });

    await test.step("intended render: the API renders the template against the issue", async () => {
      const rendered = await serverPromptPreview(
        workspaceSlug,
        "coding-task",
        { issue_id: targets.issueId },
        ownerSession
      );
      expect(rendered.prompt).toContain(targets.issueName);
      const review = await serverPromptPreview(workspaceSlug, "review", { issue_id: targets.issueId }, ownerSession);
      expect(review.prompt).toContain(targets.issueName);
      const missing = await serverPromptPreviewStatus(
        workspaceSlug,
        "coding-task",
        { issue_id: "00000000-0000-0000-0000-000000000000" },
        ownerSession
      );
      expect(missing.status).toBe(404);
    });

    await test.step("bug: the install-targeted submit fails inline on CSRF", async () => {
      await driver.promptsReceiptToggle("scheduler");
      expect(await driver.promptsSavedPreviewVisible("scheduler")).toBe(true);
      await driver.promptsSavedPreviewSubmit("scheduler", targets.bindingId);
      await expect
        .poll(async () => (await driver.promptsSavedPreviewResult("scheduler")).error ?? "", { timeout: 30_000 })
        .not.toBe("");
      const result = await driver.promptsSavedPreviewResult("scheduler");
      expect(result.error).toContain("CSRF Failed");
      expect(result.prompt).toBeNull();
      const rendered = await serverPromptPreview(
        workspaceSlug,
        "scheduler",
        { binding_id: targets.bindingId },
        ownerSession
      );
      expect(rendered.prompt.length).toBeGreaterThan(0);
      const missing = await serverPromptPreviewStatus(
        workspaceSlug,
        "scheduler",
        { binding_id: "00000000-0000-0000-0000-000000000000" },
        ownerSession
      );
      expect(missing.status).toBe(404);
    });

    await test.step("members see no saved-preview form and the API refuses their default preview", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsSavedPreviewVisible("coding-task")).toBe(false);
      const refused = await serverPromptPreviewStatus(
        workspaceSlug,
        "coding-task",
        { issue_id: targets.issueId },
        member.session
      );
      expect(refused.status).toBe(403);
    });
  }
);
