// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-748): the cloud-only estimates halves — the
// time family (EST-009), saved-system editing (EST-014), single-value
// deletion with reassignment (EST-015) and the cloud side of the edition
// matrix (EST-029). Rows: EST-009, EST-014, EST-015, EST-029.
//
// new-project only (NEWFRONT-984 pattern): the OSS oracle provably lacks
// these surfaces. Run against the cloud-edition web_new build with
// PARITY_NEW_EDITION=cloud; the OSS and desktop proofs exclude this file
// with --grep-invert "cloud:".
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateEstimate,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverEstimateDetailOrNull,
  serverEstimatesList,
  serverHistory,
  serverIssue,
  serverPatchEstimateSystem,
  serverPatchIssue,
  serverSetProjectEstimate,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** History entries on an issue naming the estimate field (celery-fed; poll for them). */
async function estimateHistoryEntries(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  session: string
): Promise<{ field: string; verb: string; oldValue: string | null; newValue: string | null }[]> {
  const probe = await serverHistory(workspaceSlug, projectId, issueId, session, "?activity_type=issue-property");
  return probe.entries
    .filter((entry) => String(entry.field ?? "").startsWith("estimate_"))
    .map((entry) => ({
      field: String(entry.field),
      verb: String(entry.verb ?? ""),
      oldValue: entry.old_value === null || entry.old_value === undefined ? null : String(entry.old_value),
      newValue: entry.new_value === null || entry.new_value === undefined ? null : String(entry.new_value),
    }));
}

test(
  specTitle(["EST-009"], "cloud: the cloud dialog offers the time family and creates a time system"),
  { tag: specTags(["EST-009"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud halves pin the web_new cloud build only");
    test.setTimeout(600_000);
    const tag = `NF748C time ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N748C"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await driver.estimatesOpenCreateDialog();

      await test.step("all three families are offered and enabled", async () => {
        const options = await driver.estimatesFamilyOptions();
        expect(options.map((option) => option.value)).toEqual(["points", "categories", "time"]);
        expect(options.every((option) => !option.disabled)).toBe(true);
      });

      await test.step("a time system creates through the Hours starter", async () => {
        await driver.estimatesSelectFamily("time");
        await driver.estimatesPickStarter("Hours");
        // Time stores raw minutes and renders them humanized.
        expect(await driver.estimatesStepTwoValues()).toEqual(["1m", "2m", "3m", "4m", "5m", "6m"]);
        await driver.estimatesSubmit();
        expect(await driver.estimatesDialogOpen()).toBe(false);
        await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe("Time");
      });

      await test.step("the server stores the time system with its raw values", async () => {
        const created = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find(
          (row) => row.name === "Time"
        );
        expect(created).not.toBeUndefined();
        expect(created?.lastUsed).toBe(true);
        expect((created?.points ?? []).map((point) => point.value).sort()).toEqual(
          ["1", "2", "3", "4", "5", "6"].sort()
        );
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-014"], "cloud: saved-system editing renames and rewrites values through the update dialog"),
  { tag: specTags(["EST-014"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud halves pin the web_new cloud build only");
    test.setTimeout(600_000);
    const tag = `NF748C edit ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N748C"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await test.step("the active row offers the edit control next to deletion", async () => {
        expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(true);
        expect(await driver.estimatesActiveRowButtonCount()).toBe(2);
      });

      await test.step("the update dialog opens over the saved values", async () => {
        await driver.estimatesActiveRowEditClick();
        expect(await driver.estimatesUpdateDialogOpen()).toBe(true);
        expect(await driver.estimatesUpdateDialogValues()).toEqual(["1", "2"]);
      });

      await test.step("renaming and rewriting values saves in one update", async () => {
        await driver.estimatesUpdateDialogSetName(`${tag} renamed`);
        await driver.estimatesUpdateDialogEditValue(0, "3");
        await driver.estimatesUpdateDialogSave();
        expect(await driver.estimatesUpdateDialogOpen()).toBe(false);
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("updated");
      });

      await test.step("the rename and values persist server-side", async () => {
        await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} renamed`);
        const detail = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
        expect(detail?.name).toBe(`${tag} renamed`);
        expect((detail?.points ?? []).map((point) => point.value).sort()).toEqual(["2", "3"]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-015"], "cloud: removing a value moves affected work items to its replacement"),
  { tag: specTags(["EST-015"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud halves pin the web_new cloud build only");
    test.setTimeout(600_000);
    const tag = `NF748C move ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N748C"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      // Distinct positions first: bulk creation leaves every key at
      // zero, which cannot prove gapless renumbering.
      const created = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      const positioned = await serverPatchEstimateSystem(
        seed.workspaceSlug,
        projectId,
        system.id,
        {
          estimate_points: (created?.points ?? []).map((point, index) => ({
            id: point.id,
            key: index + 1,
            value: point.value,
          })),
        },
        session
      );
      expect(positioned.status).toBe(200);
      const detail = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      const byKey = (key: number) => (detail?.points ?? []).find((point) => point.key === key);
      const doomed = byKey(2);
      const survivor = byKey(3);
      expect(doomed?.id).toBeDefined();
      expect(survivor?.id).toBeDefined();
      await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: doomed?.id }, session);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
      await driver.estimatesActiveRowEditClick();
      expect(await driver.estimatesUpdateDialogOpen()).toBe(true);

      await test.step("the removal asks for a reassignment choice", async () => {
        await driver.estimatesUpdateDialogRemoveValue(1);
        expect(await driver.estimatesUpdateDialogRemovalTitle()).toBe("Remove 2?");
      });

      await test.step("choosing the replacement stages the move", async () => {
        await driver.estimatesUpdateDialogChooseMove("3");
        await driver.estimatesUpdateDialogConfirmRemoval();
        expect(await driver.estimatesUpdateDialogValues()).toEqual(["1", "3"]);
        await driver.estimatesUpdateDialogSave();
        expect(await driver.estimatesUpdateDialogOpen()).toBe(false);
      });

      await test.step("the item now carries the replacement value", async () => {
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBe(survivor?.id);
      });

      await test.step("the item records a history entry", async () => {
        await expect
          .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), {
            timeout: 90_000,
          })
          .toEqual(
            expect.arrayContaining([
              expect.objectContaining({ field: "estimate_points", verb: "updated", newValue: survivor?.value }),
            ])
          );
      });

      await test.step("remaining values renumber without gaps", async () => {
        const after = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
        expect((after?.points ?? []).map((point) => point.id)).not.toContain(doomed?.id);
        expect((after?.points ?? []).map((point) => point.key).sort()).toEqual([1, 2]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-015"], "cloud: removing a value with clearing empties affected work items"),
  { tag: specTags(["EST-015"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud halves pin the web_new cloud build only");
    test.setTimeout(600_000);
    const tag = `NF748C clear ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N748C"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      const detail = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      const doomed = (detail?.points ?? []).find((point) => point.value === "2");
      expect(doomed?.id).toBeDefined();
      await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: doomed?.id }, session);
      // Baseline first: the assignment itself records an entry, which
      // also proves the worker is alive for what follows.
      await expect
        .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), { timeout: 90_000 })
        .toEqual(
          expect.arrayContaining([
            expect.objectContaining({ field: "estimate_points", verb: "updated", newValue: doomed?.value }),
          ])
        );
      const baseline = await estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
      await driver.estimatesActiveRowEditClick();
      await driver.estimatesUpdateDialogRemoveValue(1);
      await driver.estimatesUpdateDialogChooseClear();
      await driver.estimatesUpdateDialogConfirmRemoval();
      expect(await driver.estimatesUpdateDialogValues()).toEqual(["1", "3"]);
      await driver.estimatesUpdateDialogSave();
      expect(await driver.estimatesUpdateDialogOpen()).toBe(false);

      await test.step("the item's estimate is cleared", async () => {
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBeNull();
      });

      await test.step("no history entry records the clearing (bug NEWFRONT-345)", async () => {
        // The worker crashes on the empty replacement (NEWFRONT-345),
        // so the delete adds no entry. Pin that over a bounded window
        // exactly like the OSS bug: scenario: the with-replacement
        // path records its entry well inside it. If the worker is
        // fixed this fails — unmark the scenario and assert the
        // removed-entry instead.
        const deadline = Date.now() + 45_000;
        let entries = baseline;
        while (Date.now() < deadline) {
          entries = await estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session);
          if (entries.length > baseline.length) break;
          await new Promise((resolve) => setTimeout(resolve, 5000));
        }
        expect(entries).toEqual(baseline);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-029"], "cloud: the cloud build offers the edition's full estimates subset"),
  { tag: specTags(["EST-029"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud halves pin the web_new cloud build only");
    test.setTimeout(600_000);
    const tag = `NF748C matrix ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N748C"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      // The add control lives in the empty state only, so the dialog
      // half runs before any system exists.
      await test.step("the dialog offers all three families", async () => {
        await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        await driver.estimatesOpenCreateDialog();
        const options = await driver.estimatesFamilyOptions();
        expect(options.map((option) => option.value)).toEqual(["points", "categories", "time"]);
        expect(options.every((option) => !option.disabled)).toBe(true);
        await driver.estimatesDialogCancel();
        expect(await driver.estimatesDialogOpen()).toBe(false);
      });

      await test.step("the active row offers editing next to deletion", async () => {
        const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
        await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
        await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
        expect(await driver.estimatesActiveRowButtonCount()).toBe(2);
        expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
