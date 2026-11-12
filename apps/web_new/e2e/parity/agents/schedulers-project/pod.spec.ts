// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): each install runs on the project default
// pod unless given an override; the selector lists the project's pods with
// the default marked, disables while pods load, and resets a stale saved
// pod to the default instead of failing the save.
// Row: AGT-019.
import { test, expect } from "../../fixtures";
import {
  createPod,
  deletePod,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  projectPods,
  serverBindingDetail,
  serverBindings,
  serverCreateBinding,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-019"];

/** Poll visible toasts until one carries `title`. */
async function waitToast(
  driver: { schedulerVisibleToasts: () => Promise<{ title: string; message: string }[]> },
  title: string
): Promise<void> {
  await expect
    .poll(async () => (await driver.schedulerVisibleToasts()).some((toast) => toast.title === title), {
      timeout: 30_000,
    })
    .toBe(true);
}

test(
  specTitle(ROWS, "pod override persists; stale pods reset to default, selector gates on load"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag19"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt19-main-${workspaceSlug}`;

    const project = await test.step("owner prepares a project, a pod and a definition", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT19 Project ${tag}`,
        parityProjectIdentifier("AG19")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT19 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      return created;
    });
    const rig = await test.step("owner adds a project pod", async () => {
      const pods = await projectPods(project.id, ownerSession);
      const name = `agt19rig${tag.slice(0, 8)}`;
      const existing = pods.find((row) => row.name.endsWith(name));
      if (existing !== undefined) return existing;
      return createPod(project.id, ownerSession, name);
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT19 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the install list", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
    });

    await test.step("the selector disables while pods load", async () => {
      expect(await driver.schedulerPodLoadingObserved()).toBe(true);
    });

    await test.step("the selector lists project pods with the default marked and preselected", async () => {
      const server = await projectPods(project.id, ownerSession);
      expect(server.some((row) => row.id === rig.id)).toBe(true);
      await driver.schedulerOpenProjectInstall();
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
      await expect
        .poll(async () => (await driver.schedulerPodOptions()).map((row) => row.value), { timeout: 30_000 })
        .toContain(rig.id);
      const options = await driver.schedulerPodOptions();
      expect(options.map((row) => row.value).sort()).toEqual(["", ...server.map((row) => row.id)].sort());
      expect(options.find((row) => row.value === "")?.label).toBe("Project default pod");
      expect(options.find((row) => row.value === "")?.selected).toBe(true);
      const flagged = server.find((row) => row.is_default);
      expect(flagged).toBeDefined();
      expect(options.find((row) => row.value === flagged?.id)?.label).toContain("(default)");
    });

    await test.step("installing with an override persists the pod", async () => {
      await driver.schedulerProjectInstallSelect(handle);
      await driver.schedulerPodSelect(rig.id);
      await driver.schedulerProjectInstallSubmit();
      await waitToast(driver, "Scheduler installed");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const installed = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      expect(installed.pod).toBe(rig.id);
      expect(installed.pod_name).toBe(rig.name);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Pod")?.value).toBe(rig.name);
    });

    await test.step("a stale saved pod resets to the default instead of failing", async () => {
      await deletePod(rig.id, ownerSession);
      expect((await projectPods(project.id, ownerSession)).some((row) => row.id === rig.id)).toBe(false);
      expect((await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession)).pod).toBe(rig.id);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerProjectOpenEdit(handle);
      // The selector no longer offers the deleted pod, and the form already
      // normalized to the default before the save runs.
      const options = await driver.schedulerPodOptions();
      expect(options.map((row) => row.value)).not.toContain(rig.id);
      expect((await driver.schedulerProjectEditValues()).pod).toBe("");
      await driver.schedulerProjectEditSubmit();
      await waitToast(driver, "Install updated");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      const installed = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      expect(installed.pod).toBeNull();
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Pod")?.value).toBe("(default pod)");
    });

    await test.step("leaving the default persists no override", async () => {
      const spare = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt19-spare-${workspaceSlug}`,
        name: "AGT19 Spare Definition",
        prompt: "Installed on the project default.",
        is_enabled: true,
      });
      const rows = await serverBindings(workspaceSlug, project.id, ownerSession);
      if (!rows.some((row) => row.scheduler === spare.id)) {
        await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
          scheduler: spare.id,
          project: project.id,
          dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        });
      }
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler === spare.id
      );
      expect(found?.pod).toBeNull();
    });
  }
);
