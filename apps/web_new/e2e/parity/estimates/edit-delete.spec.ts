// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-324): saved-system edit (EST-014), single
// saved-value delete with reassignment (EST-015), whole-system delete
// (EST-016), and the server-side rules matrix (EST-025).
// Rows: EST-014–EST-016 + EST-025.
//
// The parity stack builds OSS apps/web only, and the OSS build ships
// placeholders where the cloud build edits: the saved-system update
// dialog and the single-value delete confirmation each render nothing
// (verified by reading the ce/ stubs: both return empty fragments), and
// saved-system rows offer a single trash control. EST-014/EST-015
// therefore assert the OSS absence live — the
// issues/edition-stubs.spec.ts tradition — and prove the API contract
// underneath server-side, since the endpoints serve every edition.
// EST-016 runs end to end in OSS. EST-025 is API-level throughout.
//
// One browser role per test: the shared sign-in helper keeps an
// existing session, so switching users mid-test would silently keep
// the first session.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateArchivedEstimate,
  serverCreateEstimate,
  serverCreateEstimatePoint,
  serverCreateEstimateRaw,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverDeleteEstimatePoint,
  serverEstimateDetailOrNull,
  serverEstimatesList,
  serverHistory,
  serverIssue,
  serverPatchEstimatePoint,
  serverPatchEstimateSystem,
  serverPatchIssue,
  serverProjectEstimateId,
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
  specTitle(["EST-014"], "OSS rows offer deletion only — no working edit dialog"),
  { tag: specTags(["EST-014"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF324 rows ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await test.step("the active row carries a single control and no edit affordance", async () => {
        expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
        expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(false);
        expect(await page.locator("body").innerText()).not.toMatch(/update estimate/i);
      });

      await test.step("the single control opens the whole-system confirmation, not an update dialog", async () => {
        await driver.estimatesActiveRowDeleteClick();
        await expect
          .poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 })
          .toContain("Delete");
        expect(await driver.estimatesEditDeleteConfirmDialogTitle()).not.toMatch(/update/i);
        await driver.estimatesConfirmDialogCancel();
        expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
      });

      // Cancelling kept everything: the system still lists server-side.
      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-014"], "PATCH with a full point set persists across a refetch"),
  { tag: specTags(["EST-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF324 patch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      const before = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      expect(before?.points).toHaveLength(3);

      const renamed = `${tag} renamed`;
      const patched = await serverPatchEstimateSystem(
        seed.workspaceSlug,
        projectId,
        system.id,
        {
          estimate: { name: renamed },
          estimate_points: (before?.points ?? []).map((point, index) => ({
            id: point.id,
            key: index + 1,
            value: `N${index + 1}`,
          })),
        },
        session
      );
      expect(patched.status).toBe(200);

      await test.step("a refetch shows the renamed system with the reworked values", async () => {
        const after = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
        expect(after?.name).toBe(renamed);
        expect((after?.points ?? []).map((point) => point.value).sort()).toEqual(["N1", "N2", "N3"]);
        expect((after?.points ?? []).map((point) => point.key).sort()).toEqual([1, 2, 3]);
      });

      await test.step("reopening the screen shows the persisted rename", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(renamed);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-014"], "system updates with no values are refused server-side"),
  { tag: specTags(["EST-014"]) },
  async ({ seed }) => {
    const tag = `NF324 empty ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      const before = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);

      const emptied = await serverPatchEstimateSystem(
        seed.workspaceSlug,
        projectId,
        system.id,
        { estimate: { name: `${tag} renamed` }, estimate_points: [] },
        session
      );
      expect(emptied.status).toBe(400);

      const missing = await serverPatchEstimateSystem(
        seed.workspaceSlug,
        projectId,
        system.id,
        { estimate: { name: `${tag} renamed` } },
        session
      );
      expect(missing.status).toBe(400);

      // Both refusals stored nothing: the refetch matches the snapshot.
      expect(await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session)).toEqual(before);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-015"], "OSS offers no single-value confirmation"),
  { tag: specTags(["EST-015"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF324 values ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await test.step("no value-level control exists to open a confirmation from", async () => {
        // Saved-system editing is the only route to value rows, and OSS
        // ships none of it: one row button, no edit entry, no per-value
        // confirmation markers anywhere on the screen.
        expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
        expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(false);
        expect(await page.locator("body").innerText()).not.toMatch(/replace.*estimate|move.*to another value/i);
      });

      await test.step("the row control opens whole-system delete, never a value dialog", async () => {
        await driver.estimatesActiveRowDeleteClick();
        await expect
          .poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 })
          .toContain("Delete");
        await driver.estimatesConfirmDialogCancel();
        expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
      });

      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-015"], "deleting a value with a replacement moves affected items"),
  { tag: specTags(["EST-015"]) },
  async ({ seed }) => {
    const tag = `NF324 move ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const first = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} one`, session);
    const second = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} two`, session);
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

      await serverPatchIssue(seed.workspaceSlug, projectId, first.id, { estimate_point: doomed?.id }, session);
      await serverPatchIssue(seed.workspaceSlug, projectId, second.id, { estimate_point: doomed?.id }, session);

      const deleted = await serverDeleteEstimatePoint(
        seed.workspaceSlug,
        projectId,
        system.id,
        doomed?.id ?? "",
        session,
        survivor?.id
      );
      expect(deleted.status).toBe(200);

      await test.step("both items now carry the replacement value", async () => {
        for (const issue of [first, second]) {
          await expect
            .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
              timeout: 30_000,
            })
            .toBe(survivor?.id);
        }
      });

      await test.step("each affected item records a history entry", async () => {
        for (const issue of [first, second]) {
          await expect
            .poll(() => estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session), {
              timeout: 90_000,
            })
            .toEqual(
              expect.arrayContaining([
                expect.objectContaining({ field: "estimate_points", verb: "updated", newValue: survivor?.value }),
              ])
            );
        }
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
  specTitle(["EST-015"], "deleting a value without a replacement clears affected items"),
  { tag: specTags(["EST-015"]) },
  async ({ seed }) => {
    const tag = `NF324 clear ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} item`, session);
    try {
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
      const doomed = (detail?.points ?? []).find((point) => point.key === 1);
      expect(doomed?.id).toBeDefined();

      await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: doomed?.id }, session);
      const deleted = await serverDeleteEstimatePoint(
        seed.workspaceSlug,
        projectId,
        system.id,
        doomed?.id ?? "",
        session
      );
      expect(deleted.status).toBe(200);

      await test.step("the item's value is emptied", async () => {
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBeNull();
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
  specTitle(["EST-015"], "bug: deleting a value without a replacement records no history entry (NEWFRONT-345)"),
  { tag: specTags(["EST-015"]) },
  async ({ seed }) => {
    const tag = `NF324 bughist ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} item`, session);
    try {
      const created = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      const doomed = created?.points[0];
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

      const deleted = await serverDeleteEstimatePoint(
        seed.workspaceSlug,
        projectId,
        system.id,
        doomed?.id ?? "",
        session
      );
      expect(deleted.status).toBe(200);
      await expect
        .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
          timeout: 30_000,
        })
        .toBeNull();

      // The worker crashes on the empty replacement (NEWFRONT-345), so
      // the delete adds no entry. Pin that over a bounded window: the
      // with-replacement path records its entry well inside it. If the
      // worker is fixed this fails — unmark the scenario and assert
      // the removed-entry instead.
      const deadline = Date.now() + 45_000;
      let entries = baseline;
      while (Date.now() < deadline) {
        entries = await estimateHistoryEntries(seed.workspaceSlug, projectId, issue.id, session);
        if (entries.length > baseline.length) break;
        await new Promise((resolve) => setTimeout(resolve, 5000));
      }
      expect(entries).toEqual(baseline);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-016"], "deleting the whole system strips values and unlinks the project"),
  { tag: specTags(["EST-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF324 gone ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} item`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: system.points[0]?.id }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await test.step("the confirmation warns values are stripped permanently", async () => {
        await driver.estimatesActiveRowDeleteClick();
        await expect
          .poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 })
          .toContain("Delete");
        const body = await driver.estimatesEditDeleteConfirmDialogBody();
        expect(body).toContain(`${tag} prime`);
        expect(body).toContain("permanently");
      });

      await test.step("confirming removes the system with a success notice", async () => {
        await driver.estimatesEditDeleteConfirmDialogConfirm();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("deleted");
        await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toBe("");
      });

      await test.step("the system is gone, values stripped, project unlinked", async () => {
        expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).not.toContain(
          system.id
        );
        await expect.poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 30_000,
          })
          .toBeNull();
        expect(await driver.estimatesActiveSystemName()).toBe("");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-016"], "cancelling the whole-system delete keeps everything"),
  { tag: specTags(["EST-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF324 keep ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} item`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { estimate_point: system.points[0]?.id }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await driver.estimatesActiveRowDeleteClick();
      await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toContain("Delete");
      await driver.estimatesConfirmDialogCancel();
      expect(await driver.estimatesConfirmDialogOpen()).toBe(false);

      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
      expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBe(system.id);
      expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point).toBe(
        system.points[0]?.id
      );
      expect(await driver.estimatesActiveSystemName()).toBe(`${tag} prime`);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-016"], "bug: deleting an unlinked system clears the linked project reference (NEWFRONT-809)"),
  { tag: specTags(["EST-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF809 split ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N809"),
      {},
      session
    );
    // The distinguishing setup: the project links an archived (dormant)
    // system while a different, flagged system is the active row. The
    // flagged row is deletable from the screen; the linked one is not.
    const kept = await serverCreateArchivedEstimate(seed.workspaceSlug, projectId, `${tag} kept`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, kept.id, session);
    const active = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      await driver.estimatesActiveRowDeleteClick();
      await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toContain("Delete");
      await driver.estimatesEditDeleteConfirmDialogConfirm();
      await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("deleted");

      await test.step("the deleted system is gone", async () => {
        expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).not.toContain(
          active.id
        );
        expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
          kept.id
        );
      });

      // The old client unlinks whenever the project has any linked
      // system, so the unrelated link is cleared too (NEWFRONT-809).
      // Intended (new app, spec §4.3 narrow rule): the reference still
      // points at the kept system. The fix shipped in the new app while
      // the oracle still pins the bug, so each leg asserts its own
      // behavior (NEWFRONT-349 precedent). If the old app is fixed this
      // fails — unmark the scenario and assert the preserved link.
      if (driver.target === "web_new") {
        await expect.poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBe(kept.id);
      } else {
        await expect.poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-016"], "a refused whole-system delete reports an error and keeps the system"),
  { tag: specTags(["EST-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF324 fail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      // Refuse the delete behind the dialog's back: confirming must
      // report the error instead of closing. Only the notice body is
      // asserted — its title names the wrong action (NEWFRONT-344), so
      // the title is deliberately not pinned.
      await driver.estimatesEditDeleteFailEstimateDelete();
      await driver.estimatesActiveRowDeleteClick();
      await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toContain("Delete");
      await driver.estimatesEditDeleteConfirmDialogConfirm();
      await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("unable to delete");
      expect(await driver.estimatesEditDeleteConfirmDialogTitle()).not.toBe("");

      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
      expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBe(system.id);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-025"], "point creation requires both position and value"),
  { tag: specTags(["EST-025"]) },
  async ({ seed }) => {
    const tag = `NF324 keys ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      const before = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);

      for (const payload of [{}, { value: "9" }, { key: 5 }]) {
        const probe = await serverCreateEstimatePoint(seed.workspaceSlug, projectId, system.id, payload, session);
        expect(probe.status).toBe(400);
      }

      // Every refusal stored nothing: the point set matches the snapshot.
      expect(await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session)).toEqual(before);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(specTitle(["EST-025"], "value text is length-capped"), { tag: specTags(["EST-025"]) }, async ({ seed }) => {
  const tag = `NF324 caps ${Date.now()}`;
  const session = await signInSession(seed.email, seed.password);
  const projectId = await serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N324"),
    {},
    session
  );
  const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
  try {
    const overlong = "x".repeat(21);

    await test.step("a point update past the cap is refused and stores nothing", async () => {
      const before = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);
      const pointId = before?.points[0]?.id ?? "";
      const probe = await serverPatchEstimatePoint(
        seed.workspaceSlug,
        projectId,
        system.id,
        pointId,
        { value: overlong },
        session
      );
      expect(probe.status).toBe(400);
      expect(await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session)).toEqual(before);
    });

    await test.step("a system create past the cap is refused and stores no points", async () => {
      // Only the refusal and the point set are pinned: the refused
      // create still leaves an empty system row behind (NEWFRONT-346),
      // and single-point creation skips the cap entirely (NEWFRONT-347).
      const listedBefore = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      const probe = await serverCreateEstimateRaw(
        seed.workspaceSlug,
        projectId,
        {
          estimate: { name: `${tag} overlong`, type: "points", last_used: false },
          estimate_points: [{ value: "1" }, { value: overlong }],
        },
        session
      );
      expect(probe.status).toBe(400);
      const listedAfter = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      const fresh = listedAfter.filter((row) => !listedBefore.some((old) => old.id === row.id));
      expect(fresh.flatMap((row) => row.points)).toHaveLength(0);
    });
  } finally {
    await serverCleanupProject(seed.workspaceSlug, projectId, session);
  }
});

test(
  specTitle(["EST-025"], "system updates with no values are rejected"),
  { tag: specTags(["EST-025"]) },
  async ({ seed }) => {
    const tag = `NF324 noval ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      const before = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session);

      const emptied = await serverPatchEstimateSystem(
        seed.workspaceSlug,
        projectId,
        system.id,
        { estimate_points: [] },
        session
      );
      expect(emptied.status).toBe(400);

      expect(await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, system.id, session)).toEqual(before);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-025"], "omitted system names are generated into a usable system"),
  { tag: specTags(["EST-025"]) },
  async ({ seed }) => {
    const tag = `NF324 named ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    try {
      const probe = await serverCreateEstimateRaw(
        seed.workspaceSlug,
        projectId,
        {
          estimate: { type: "points", last_used: false },
          estimate_points: [{ value: "1" }, { value: "2" }],
        },
        session
      );
      expect(probe.status).toBe(200);
      const body = probe.json as Record<string, unknown>;
      expect(typeof body["id"]).toBe("string");
      expect(typeof body["name"]).toBe("string");
      expect(String(body["name"] ?? "")).not.toBe("");

      // The generated system is usable: it refetches with its points
      // and links onto the project like a named one.
      const detail = await serverEstimateDetailOrNull(seed.workspaceSlug, projectId, String(body["id"]), session);
      expect(detail?.name).toBe(String(body["name"]));
      expect(detail?.points).toHaveLength(2);
      await serverSetProjectEstimate(seed.workspaceSlug, projectId, String(body["id"]), session);
      expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBe(String(body["id"]));
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-025"], "duplicate system names per project are refused"),
  { tag: specTags(["EST-025"]) },
  async ({ seed }) => {
    const tag = `NF324 dupes ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N324"),
      {},
      session
    );
    try {
      const name = `${tag} prime`;
      const first = await serverCreateEstimateRaw(
        seed.workspaceSlug,
        projectId,
        { estimate: { name, type: "points", last_used: true }, estimate_points: [{ value: "1" }, { value: "2" }] },
        session
      );
      expect(first.status).toBe(200);

      const second = await serverCreateEstimateRaw(
        seed.workspaceSlug,
        projectId,
        { estimate: { name, type: "points", last_used: false }, estimate_points: [{ value: "1" }] },
        session
      );
      expect(second.status).toBe(400);

      // The refusal stored nothing: exactly one system carries the name.
      const listed = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      expect(listed.filter((row) => row.name === name)).toHaveLength(1);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
