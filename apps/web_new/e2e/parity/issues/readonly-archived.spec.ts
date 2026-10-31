// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): read-only property surface — archived
// issues render the detail sidebar with every picker disabled while values
// stay visible. (The dedicated readonly/ components have no usages in the
// old app; the archived sidebar is the live read-only surface.)
// Rows: ISS-220 (read-only property components).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverArchiveIssue,
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
  async ({ driver, seed }) => {
    const tag = `NF122 readonly ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
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
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      if (ownedStateId !== null)
        await serverDeleteState(seed.workspaceSlug, projectId, ownedStateId, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
