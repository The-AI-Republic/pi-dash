// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): publishing a draft into a project work
// item with values, files and history carried over, and the refused
// publish of a project-less draft. Rows: DRAFT-020, DRAFT-021. Green on
// apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  serverConvertDraftStatus,
  serverCreateCycle,
  serverCreateDraft,
  serverCreateEstimate,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateModule,
  serverCreateState,
  serverDraftNames,
  serverFetchDraftStatus,
  serverFileAsset,
  serverHistory,
  serverIssue,
  serverIssueNames,
  serverIssues,
  serverSetProjectEstimate,
  serverUploadDraftAsset,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

function isoIn(days: number): string {
  const at = new Date();
  at.setDate(at.getDate() + days);
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
}

test(
  specTitle(["DRAFT-020"], "publishing converts the draft, carries values and files, and logs activity"),
  { tag: specTags(["DRAFT-020"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d20");
    const { owner, workspaceSlug, workspaceId, projectId } = harness;
    const name = `D20 Publish ${harness.tag}`;
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d20-member", ROLE.MEMBER);
    const state2 = await serverCreateState(workspaceSlug, projectId, "Doing", "started", owner.cookie);
    const label = await serverCreateLabel(workspaceSlug, projectId, `Plabel ${harness.tag}`, "#00aa55", owner.cookie);
    const cycleId = await serverCreateCycle(
      workspaceSlug,
      projectId,
      `Pcycle ${harness.tag}`,
      isoIn(-7),
      isoIn(60),
      owner.cookie
    );
    const moduleId = await serverCreateModule(workspaceSlug, projectId, `Pmodule ${harness.tag}`, owner.cookie);
    const system = await serverCreateEstimate(workspaceSlug, projectId, "Fib", ["1", "2", "3"], owner.cookie);
    await serverSetProjectEstimate(workspaceSlug, projectId, system.id, owner.cookie);
    const parentId = await serverCreateIssue(workspaceSlug, projectId, owner.cookie, `Pparent ${harness.tag}`);
    const description = `<p>Publish body ${harness.tag}</p>`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, {
      name,
      project_id: projectId,
      description_html: description,
      state_id: state2,
      priority: "high",
      label_ids: [label.id],
      assignee_ids: [member.userId],
      start_date: isoIn(1),
      target_date: isoIn(10),
      estimate_point: system.points[1]?.id,
      cycle_id: cycleId,
      module_ids: [moduleId],
      parent_id: parentId,
    });
    // A minimal valid 1x1 PNG: the asset flow accepts images only.
    const pixel = Uint8Array.from(
      atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="),
      (ch) => ch.charCodeAt(0)
    );
    const asset = await serverUploadDraftAsset(workspaceSlug, workspaceId, projectId, draft.id, owner.cookie, {
      name: "publish-pixel.png",
      mime: "image/png",
      bytes: pixel,
    });
    expect(asset.draftIssueId).toBe(draft.id);

    await test.step("publish through the edit dialog", async () => {
      await draftsOpenAs(driver, harness);
      await driver.editDraftByName(name);
      await driver.publishDraft();
      await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Draft published to project.");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).not.toContain(name);
    });

    await test.step("the work item carries every value", async () => {
      await expect
        .poll(() => serverIssueNames(workspaceSlug, projectId, owner.cookie), { timeout: 60_000 })
        .toContain(name);
      const issues = await serverIssues(workspaceSlug, projectId, owner.cookie);
      const created = issues.find((issue) => issue.name === name);
      expect(created).toBeDefined();
      const detail = await serverIssue(workspaceSlug, projectId, created?.id ?? "", owner.cookie);
      expect(detail.name).toBe(name);
      expect(detail.descriptionHtml).toContain(`Publish body ${harness.tag}`);
      expect(detail.state_id).toBe(state2);
      expect(detail.priority).toBe("high");
      expect(detail.label_ids).toContain(label.id);
      expect(detail.assignee_ids).toContain(member.userId);
      expect(detail.start_date).toBe(isoIn(1));
      expect(detail.target_date).toBe(isoIn(10));
      expect(detail.estimate_point).toBe(system.points[1]?.id);
      expect(detail.cycle_id).toBe(cycleId);
      expect(detail.module_ids).toContain(moduleId);
      expect(detail.parentId).toBe(parentId);
    });

    await test.step("the draft is gone and the file moved to the work item", async () => {
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).not.toContain(name);
      expect((await serverFetchDraftStatus(workspaceSlug, draft.id, owner.cookie)).status).toBe(404);
      const issues = await serverIssues(workspaceSlug, projectId, owner.cookie);
      const created = issues.find((issue) => issue.name === name);
      const moved = await serverFileAsset(workspaceId, asset.assetKey, owner.cookie);
      expect(moved.issueId).toBe(created?.id);
      expect(moved.draftIssueId).toBeNull();
      expect(moved.entityType).toBe("ISSUE_DESCRIPTION");
    });

    await test.step("the conversion logs issue activity", async () => {
      const issues = await serverIssues(workspaceSlug, projectId, owner.cookie);
      const created = issues.find((issue) => issue.name === name);
      await expect
        .poll(
          async () =>
            (
              await serverHistory(
                workspaceSlug,
                projectId,
                created?.id ?? "",
                owner.cookie,
                "?activity_type=issue-property"
              )
            ).entries.length,
          { timeout: 90_000 }
        )
        .toBeGreaterThanOrEqual(1);
    });
  }
);

test(
  specTitle(["DRAFT-021"], "publishing a project-less draft is refused and keeps the draft"),
  { tag: specTags(["DRAFT-021"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d21");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D21 No Project ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name });

    // bug: the draft counts toward the server total but renders no row
    // (NEWFRONT-305); the row should render with an empty project slot.
    await test.step("the project-less draft renders no row (NEWFRONT-305)", async () => {
      await draftsOpenAs(driver, harness);
      expect(await driver.draftRowNames()).not.toContain(name);
    });

    await test.step("converting is refused and nothing is created", async () => {
      const attempt = await serverConvertDraftStatus(workspaceSlug, draft.id, owner.cookie, {});
      expect(attempt.status).toBe(400);
      expect(attempt.bodyText).toContain("Project is required");
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).toContain(name);
      expect(await serverIssueNames(workspaceSlug, projectId, owner.cookie)).not.toContain(name);
    });
  }
);
