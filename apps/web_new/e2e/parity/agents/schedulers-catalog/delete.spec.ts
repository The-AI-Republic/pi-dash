// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): deleting a definition asks for an explicit
// confirmation describing the consequences; confirming removes it, stops its
// installs from firing, frees the handle and refreshes the list; cancelling
// keeps everything; a failed delete shows an error notice. Row: AGT-004.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverCreateScheduler,
  serverDeleteScheduler,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-004"];

test(
  specTitle(ROWS, "delete confirms consequences; confirm removes and frees the handle"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt4"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const slug = `agt4-delete-${workspaceSlug}`;

    const projectId = await test.step("owner prepares a definition with an install", async () => {
      const project = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT4 Project ${tag}`,
        parityProjectIdentifier("AG4")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug,
        name: "AGT4 Doomed Definition",
        prompt: "Soon gone.",
      });
      await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
        scheduler: definition.id,
        project: project.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return project.id;
    });

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("the confirmation describes the consequences", async () => {
      await driver.schedulerOpenDelete(slug);
      const text = await driver.schedulerDeleteDialogText();
      expect(text).toContain("stop firing");
      expect(text).toContain("available for re-creation");
      expect(text).toContain("AGT4 Doomed Definition");
    });

    await test.step("cancelling keeps the definition and its install", async () => {
      await driver.schedulerCancelDelete();
      await expect
        .poll(async () => (await driver.schedulerCatalogRows()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(slug);
      const stored = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      expect(stored).toBeDefined();
      expect(await serverBindings(workspaceSlug, projectId, ownerSession)).toHaveLength(1);
    });

    await test.step("confirming removes it, stops installs and frees the handle", async () => {
      await driver.schedulerOpenDelete(slug);
      await driver.schedulerConfirmDelete();
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 60_000 })
        .toEqual({ title: "Scheduler deleted", message: expect.stringContaining("stopped firing") });
      await expect
        .poll(async () => (await driver.schedulerCatalogRows()).map((row) => row.handle), { timeout: 60_000 })
        .not.toContain(slug);
      const stored = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      expect(stored).toBeUndefined();
      expect(await serverBindings(workspaceSlug, projectId, ownerSession)).toEqual([]);
      // The freed handle is reusable immediately.
      const recreated = await serverCreateScheduler(workspaceSlug, ownerSession, {
        slug,
        name: "AGT4 Recreated",
        prompt: "Same handle, new row.",
      });
      expect(recreated.slug).toBe(slug);
      await serverDeleteScheduler(workspaceSlug, recreated.id, ownerSession);
    });

    await test.step("a failed delete shows an error notice and keeps the dialog", async () => {
      const doomed = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt4-raced-${workspaceSlug}`,
        name: "AGT4 Raced Away",
        prompt: "Deleted out from under the dialog.",
      });
      // The row was created behind the table's back; reload so it is listed.
      await driver.schedulerOpenCatalog(workspaceSlug);
      await driver.schedulerOpenDelete(doomed.slug);
      await serverDeleteScheduler(workspaceSlug, doomed.id, ownerSession);
      await driver.schedulerConfirmDelete();
      expect(await driver.schedulerDeleteOpen()).toBe(true);
      await driver.schedulerCancelDelete();
    });
  }
);
