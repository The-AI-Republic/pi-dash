// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): authoring a new definition and installing
// it in a single project-side flow; the modal opens on the create path when
// nothing is installable; handle-uniqueness conflicts map inline; a failed
// install after a created definition flips to the install path with the new
// entry preselected so nothing is orphaned; hidden-path required fields
// never block the visible path; the create path stays hidden without
// workspace-admin rights.
// Row: AGT-009.
// Note: the modal picks its initial path from data that may still be loading
// when it mounts, so cold opens navigate to the needed path; the automatic
// create-path opening is asserted only after the project's data has settled
// in the UI.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-009"];

/** Poll visible toasts until one carries `title`; resolves with its message. */
async function toastMessage(
  driver: { schedulerVisibleToasts: () => Promise<{ title: string; message: string }[]> },
  title: string
): Promise<string> {
  let message = "";
  await expect
    .poll(
      async () => {
        const found = (await driver.schedulerVisibleToasts()).find((toast) => toast.title === title);
        message = found?.message ?? "";
        return message;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return message;
}

test(
  specTitle(ROWS, "author-and-install flows in one submit; conflicts map inline, failures flip to install"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt9"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const createdHandle = `agt9-authored-${workspaceSlug}`;
    const flippedHandle = `agt9-flipped-${workspaceSlug}`;

    const project = await test.step("owner prepares a project", async () =>
      ensureProject(workspaceSlug, ownerSession, `AGT9 Project ${tag}`, parityProjectIdentifier("AG9")));

    await test.step("owner signs in and opens the create path", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerOpenProjectInstall();
      // The modal picks its initial path from data that may still be
      // loading on a fresh page; the create tab is always one flip away.
      if ((await driver.schedulerProjectInstallMode()) !== "create") {
        await driver.schedulerProjectInstallSelectTab("Create new");
      }
      expect(await driver.schedulerProjectInstallMode()).toBe("create");
    });

    await test.step("authoring and installing in one submit persists both rows", async () => {
      const defsBefore = await serverSchedulers(workspaceSlug, ownerSession);
      await driver.schedulerProjectCreateFillName("AGT9 Authored Definition");
      await driver.schedulerProjectCreateFillHandle(createdHandle);
      await driver.schedulerProjectCreateFillDescription("Authored from the project side.");
      await driver.schedulerProjectCreateFillPrompt("Audit this project nightly.");
      await driver.schedulerProjectInstallSubmit();
      const message = await toastMessage(driver, "Scheduler created and installed");
      expect(message).toContain("configured schedule");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const defsAfter = await serverSchedulers(workspaceSlug, ownerSession);
      expect(defsAfter.length).toBe(defsBefore.length + 1);
      const definition = defsAfter.find((row) => row.slug === createdHandle);
      expect(definition).toBeDefined();
      expect(definition?.is_enabled).toBe(true);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      const installed = bindings.find((row) => row.scheduler_slug === createdHandle);
      expect(installed).toBeDefined();
      expect(installed?.rrule).toBe("FREQ=DAILY");
      expect(installed?.enabled).toBe(true);
      expect(installed?.outcome_mode).toBe("create_issue");
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(createdHandle);
    });

    await test.step("a handle conflict maps inline and keeps the modal open", async () => {
      const defsBefore = await serverSchedulers(workspaceSlug, ownerSession);
      await driver.schedulerOpenProjectInstall();
      await driver.schedulerProjectInstallSelectTab("Create new");
      await driver.schedulerProjectCreateFillName("AGT9 Clashing Definition");
      await driver.schedulerProjectCreateFillHandle(createdHandle);
      await driver.schedulerProjectCreateFillPrompt("This handle is taken.");
      await driver.schedulerProjectInstallSubmit();
      let errors: string[] = [];
      await expect
        .poll(
          async () => {
            errors = await driver.schedulerProjectInstallErrors();
            return errors.join(" | ");
          },
          { timeout: 30_000 }
        )
        .toContain("already in use");
      expect(await driver.schedulerProjectInstallOpen()).toBe(true);
      expect((await serverSchedulers(workspaceSlug, ownerSession)).length).toBe(defsBefore.length);
    });

    await test.step("definition-ok with install-failed flips to install, preselected", async () => {
      await driver.schedulerProjectCreateFillName("AGT9 Flipped Definition");
      await driver.schedulerProjectCreateFillHandle(flippedHandle);
      await driver.schedulerProjectCreateFillPrompt("Its first install fails.");
      await driver.schedulerProjectInstallFillSchedule({ rrule: "FREQ=NOPE" });
      await driver.schedulerProjectInstallSubmit();
      const message = await toastMessage(driver, "Scheduler created but not installed");
      expect(message).toContain("workspace catalog");
      await expect.poll(() => driver.schedulerProjectInstallMode(), { timeout: 30_000 }).toBe("install");
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(flippedHandle);
      const options = await driver.schedulerProjectInstallOptions();
      expect(options.find((row) => row.handle === flippedHandle)?.selected).toBe(true);
      // The retry completes the install: nothing is orphaned.
      await driver.schedulerProjectInstallFillSchedule({ rrule: "FREQ=DAILY" });
      await driver.schedulerProjectInstallSubmit();
      await toastMessage(driver, "Scheduler installed");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      expect(bindings.filter((row) => row.scheduler_slug === flippedHandle).length).toBe(1);
      const defs = await serverSchedulers(workspaceSlug, ownerSession);
      expect(defs.filter((row) => row.slug === flippedHandle).length).toBe(1);
    });

    await test.step("the modal opens on create when nothing is installable", async () => {
      for (const row of await serverSchedulers(workspaceSlug, ownerSession)) {
        if (!row.is_enabled) continue;
        await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
          scheduler: row.id,
          project: project.id,
          dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        }).catch(() => undefined);
      }
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      // Settle the project's bindings first: the modal reads them at open
      // time to pick its initial path.
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual(
          (await serverBindings(workspaceSlug, project.id, ownerSession)).map((row) => row.scheduler_slug).sort()
        );
      await driver.schedulerOpenProjectInstall();
      expect(await driver.schedulerProjectInstallMode()).toBe("create");
      await driver.schedulerCloseProjectInstall();
    });

    await test.step("a project admin without author rights installs but never authors", async () => {
      const fresh = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt9-spare-${workspaceSlug}`,
        name: "AGT9 Spare Definition",
        prompt: "Left installable for the project admin.",
        is_enabled: true,
      });
      const admin = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.ADMIN, "parity-agt9a");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(admin.email, admin.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual(
          (await serverBindings(workspaceSlug, project.id, ownerSession)).map((row) => row.scheduler_slug).sort()
        );
      // A fresh page may mount the modal before definitions resolve (the
      // dead-end transient); reopen until the install path settles.
      for (let attempt = 0; ; attempt++) {
        await driver.schedulerOpenProjectInstall();
        if ((await driver.schedulerProjectInstallMode()) === "install") break;
        if (attempt >= 4) throw new Error("[parity] the install path never settled.");
        await driver.schedulerCloseProjectInstall();
      }
      expect(await driver.schedulerProjectInstallTabs()).toEqual([]);
      // The untouched create path (empty required fields) never blocks the
      // visible install path.
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(`agt9-spare-${workspaceSlug}`);
      await driver.schedulerProjectInstallSelect(`agt9-spare-${workspaceSlug}`);
      await driver.schedulerProjectInstallSubmit();
      await toastMessage(driver, "Scheduler installed");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      expect(bindings.some((row) => row.scheduler === fresh.id)).toBe(true);
    });
  }
);
