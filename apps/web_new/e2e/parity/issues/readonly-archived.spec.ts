// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): read-only property surface — archived
// issues render the detail sidebar with every picker disabled while values
// stay visible. (The dedicated readonly/ components have no usages in the
// old app; the archived sidebar is the live read-only surface.)
// NEWPIDASH-141 extends the row: the sub-work-item and attachment action
// entries stay disabled too, matching the old whole-widgets disable.
// NEWPIDASH-155 extends the row further: the code-reviews attach and
// detach entries stay hidden on archived items too, matching the old
// disabled={disabled || is_archived} pass.
// Rows: ISS-220 (read-only property components).
import { test, expect } from "../fixtures";
import {
  issueBrowseKey,
  parityProjectIdentifier,
  serverArchiveIssue,
  serverAttachCodeReview,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDeleteState,
  serverIssue,
  serverPatchIssue,
  serverProjectStates,
  serverUnarchiveIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-220"], "archived issues render every sidebar picker disabled"),
  { tag: specTags(["ISS-220"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 readonly ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectCode = parityProjectIdentifier("N122");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      projectCode,
      {},
      session
    );
    // Archiving requires a completed or cancelled state; reuse a
    // project-default one when present, else mint a scenario-owned one.
    const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
    const seededDone = states.find((s) => s.group === "completed" || s.group === "cancelled");
    const ownedStateId =
      seededDone === undefined
        ? await serverCreateState(seed.workspaceSlug, projectId, `${tag} done`, "completed", session)
        : null;
    const doneStateId = seededDone?.id ?? (ownedStateId as string);
    const doneStateName =
      seededDone?.name ??
      (await serverProjectStates(seed.workspaceSlug, projectId, session)).find((s) => s.id === ownedStateId)?.name ??
      `${tag} done`;
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: doneStateId }, session);
    // Seed one review link so the Code reviews section mounts; review repo
    // names are globally unique, so mint a per-run one.
    const reviewRepo = `hello-world-nf122-${tag.replace(/[^0-9]/g, "").slice(-10)}`;
    const reviewUrl = `https://github.com/octocat/${reviewRepo}/pull/1`;
    await serverAttachCodeReview(seed.workspaceSlug, projectId, issue.id, reviewUrl, session);
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the server marks the issue archived", async () => {
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.archived_at).not.toBe(null);
      });

      await test.step("every sidebar picker is disabled but values stay visible", async () => {
        for (const label of ["State", "Assignees", "Runs on", "Priority", "Start date", "Due date", "Parent"]) {
          expect(await driver.propertyPickerDisabled(label)).toBe(true);
        }
        // The Labels row renders no trigger at all when archived (static
        // row, zero buttons) — the strongest form of read-only.
        expect(await driver.propertyRowPresent("Labels")).toBe(true);
        expect(await driver.propertyTriggerPresent("Labels")).toBe(false);
        // Values still render (read-only shows, never edits).
        await expect.poll(() => driver.propertyValueText("State"), { timeout: 15_000 }).toContain(doneStateName);
        expect(await driver.pickerOpen()).toBe(false);
      });

      // NEWPIDASH-141: the browse address (not the archives frame above,
      // which mounts no widgets) is the widget surface — an archived issue
      // keeps its add entries disabled there too.
      await test.step("the sub-work-item and attachment entries stay disabled on the browse address", async () => {
        const key = await issueBrowseKey(seed.workspaceSlug, projectId, projectCode, issue.id, session);
        await driver.openBrowseWorkItem(seed.workspaceSlug, key);
        expect(await driver.browseShowsWorkItemDetail()).toBe(true);
        // The entries mount disabled while membership is still resolving
        // and only enable once the project read lands, so a single read
        // can land in that pending window. Read them repeatedly until the
        // detail has settled; an enabled read at any point is NEWPIDASH-141.
        for (let attempt = 0; attempt < 8; attempt++) {
          expect(await driver.widgetActionDisabled("Add sub-work item")).toBe(true);
          expect(await driver.widgetActionDisabled("Attach")).toBe(true);
          if (attempt < 7) await page.waitForTimeout(1000);
        }
      });

      await test.step("the code-reviews widget stays read-only on the archived browse address", async () => {
        // The section is still visible (list, not form) on the browse
        // address; its attach and detach entries must be hidden while
        // archived. Poll for the section to settle; any live entry at any
        // point is NEWPIDASH-155.
        for (let attempt = 0; attempt < 8; attempt++) {
          expect(await driver.codeReviewsVisible()).toBe(true);
          const links = await driver.codeReviewLinks();
          expect(links).toHaveLength(1);
          expect(links[0]!.href).toBe(reviewUrl);
          expect(await driver.codeReviewAttachVisible()).toBe(false);
          expect(await driver.codeReviewDetachVisible()).toBe(false);
          if (attempt < 7) await page.waitForTimeout(1000);
        }
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      if (ownedStateId !== null)
        await serverDeleteState(seed.workspaceSlug, projectId, ownedStateId, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
