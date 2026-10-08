// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): uninstalling asks for an explicit
// confirmation explaining that firing stops while the workspace definition
// survives; cancel keeps the install; confirm removes it and the definition
// stays reinstallable; a detail-page uninstall returns to the install list;
// a failed uninstall surfaces an error notice and keeps the dialog open.
// Row: AGT-012.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverDeleteBinding,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-012"];

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
  specTitle(ROWS, "uninstall confirms consequences; definition survives and stays reinstallable"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag12"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const firstHandle = `agt12-first-${workspaceSlug}`;
    const secondHandle = `agt12-second-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with two installs", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT12 Project ${tag}`,
        parityProjectIdentifier("AG12")
      );
      const dtstart = new Date(Date.now() + 24 * 3600_000).toISOString();
      for (const [slug, name] of [
        [firstHandle, "AGT12 First Definition"],
        [secondHandle, "AGT12 Second Definition"],
      ]) {
        const definition = await ensureScheduler(workspaceSlug, ownerSession, {
          slug,
          name,
          prompt: "Audit this project nightly.",
          is_enabled: true,
        });
        const already = (await serverBindings(workspaceSlug, created.id, ownerSession)).some(
          (row) => row.scheduler === definition.id
        );
        if (!already) {
          await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
            scheduler: definition.id,
            project: created.id,
            dtstart,
            rrule: "FREQ=DAILY",
          });
        }
      }
      return created;
    });

    await test.step("owner signs in and opens the install list", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
    });

    await test.step("the confirmation explains the consequences; cancel keeps the install", async () => {
      await driver.schedulerProjectOpenUninstall(firstHandle);
      const text = await driver.schedulerUninstallDialogText();
      expect(text).toContain("Uninstall scheduler?");
      expect(text).toContain("stops firing on this project");
      expect(text).toContain("unaffected");
      expect(text).toContain("AGT12 First Definition");
      await driver.schedulerCancelUninstall();
      expect(await driver.schedulerUninstallOpen()).toBe(false);
      expect(
        (await serverBindings(workspaceSlug, project.id, ownerSession)).some(
          (row) => row.scheduler_slug === firstHandle
        )
      ).toBe(true);
    });

    await test.step("confirm removes the install; the definition survives", async () => {
      await driver.schedulerProjectOpenUninstall(firstHandle);
      await driver.schedulerConfirmUninstall();
      const message = await toastMessage(driver, "Scheduler uninstalled");
      expect(message).toContain("until reinstalled");
      expect(await driver.schedulerUninstallOpen()).toBe(false);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle), { timeout: 30_000 })
        .not.toContain(firstHandle);
      expect(
        (await serverBindings(workspaceSlug, project.id, ownerSession)).some(
          (row) => row.scheduler_slug === firstHandle
        )
      ).toBe(false);
      expect((await serverSchedulers(workspaceSlug, ownerSession)).map((row) => row.slug)).toContain(firstHandle);
    });

    await test.step("the freed definition stays reinstallable", async () => {
      await driver.schedulerOpenProjectInstall();
      expect(await driver.schedulerProjectInstallMode()).toBe("install");
      const options = await driver.schedulerProjectInstallOptions();
      expect(options.map((row) => row.handle)).toContain(firstHandle);
      await driver.schedulerCloseProjectInstall();
    });

    await test.step("a failed uninstall surfaces an error and keeps the dialog open", async () => {
      // Remove the install behind the dialog's back: the confirm then 404s.
      const target = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === secondHandle
      );
      expect(target).toBeDefined();
      if (target === undefined) throw new Error("[parity] expected the second install.");
      await driver.schedulerProjectOpenUninstall(secondHandle);
      await serverDeleteBinding(workspaceSlug, project.id, target.id, ownerSession);
      await driver.schedulerConfirmUninstall();
      await toastMessage(driver, "Something went wrong");
      expect(await driver.schedulerUninstallOpen()).toBe(true);
      await driver.schedulerCancelUninstall();
    });

    await test.step("a detail-page uninstall returns to the install list", async () => {
      const fresh = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt12-third-${workspaceSlug}`,
        name: "AGT12 Third Definition",
        prompt: "Uninstalled from its detail page.",
        is_enabled: true,
      });
      const installed = await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
        scheduler: fresh.id,
        project: project.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, installed.id);
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe("AGT12 Third Definition");
      await driver.schedulerBindingOpenUninstall();
      await driver.schedulerConfirmUninstall();
      await toastMessage(driver, "Scheduler uninstalled");
      // The detail navigates back to the install list on success.
      await expect.poll(() => driver.schedulerProjectRows(), { timeout: 30_000 }).toEqual([]);
      expect(await driver.schedulerProjectEmptyVisible()).toBe(true);
      expect(await serverBindings(workspaceSlug, project.id, ownerSession)).toEqual([]);
    });
  }
);
