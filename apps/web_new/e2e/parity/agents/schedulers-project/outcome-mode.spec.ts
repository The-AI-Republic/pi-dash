// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): each install chooses what its runs do
// with their findings — file issues, apply fixes for review, or file plus
// delegate — with help text updating per option and issue-filing as the
// default; one definition behaves differently across projects; the edit
// dialog carries the same field.
// Row: AGT-018.
import { test, expect } from "../../fixtures";
import { ensureProject, ensureScheduler, parityProjectIdentifier, serverBindings } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-018"];

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
  specTitle(ROWS, "outcome mode defaults to issue-filing and varies per install"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag18"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt18-main-${workspaceSlug}`;

    const projects = await test.step("owner prepares two projects and one definition", async () => {
      const first = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT18 First ${tag}`,
        parityProjectIdentifier("AG18")
      );
      const second = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT18 Second ${tag}`,
        parityProjectIdentifier("AG1S")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT18 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      return { first, second };
    });

    await test.step("owner signs in and opens the install path", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, projects.first.id);
      await driver.schedulerOpenProjectInstall();
      // A fresh page may still be loading data when the modal mounts, so
      // navigate to the install path instead of asserting the initial one.
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
    });

    await test.step("the field offers three modes with per-option help, defaulting to issues", async () => {
      const state = await driver.schedulerOutcomeState();
      expect(state.options.map((row) => row.label)).toEqual(["Create issues", "Apply fix", "Fix & open for review"]);
      expect(state.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Create issues"]);
      expect(state.help).toContain("open issue");
      await driver.schedulerOutcomeSelect("Apply fix");
      const applied = await driver.schedulerOutcomeState();
      expect(applied.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Apply fix"]);
      expect(applied.help).toContain("pull request");
      await driver.schedulerOutcomeSelect("Fix & open for review");
      const delegated = await driver.schedulerOutcomeState();
      expect(delegated.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Fix & open for review"]);
      expect(delegated.help).toContain("In Progress");
    });

    await test.step("installing with fix mode persists it on the first project", async () => {
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(handle);
      await driver.schedulerProjectInstallSelect(handle);
      await driver.schedulerOutcomeSelect("Apply fix");
      await driver.schedulerProjectInstallSubmit();
      await waitToast(driver, "Scheduler installed");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const installed = (await serverBindings(workspaceSlug, projects.first.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      expect(installed?.outcome_mode).toBe("apply_fix");
    });

    await test.step("the same definition files issues on the second project", async () => {
      await driver.schedulerOpenProjectList(workspaceSlug, projects.second.id);
      await driver.schedulerOpenProjectInstall();
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(handle);
      await driver.schedulerProjectInstallSelect(handle);
      const state = await driver.schedulerOutcomeState();
      expect(state.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Create issues"]);
      await driver.schedulerProjectInstallSubmit();
      await waitToast(driver, "Scheduler installed");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const installed = (await serverBindings(workspaceSlug, projects.second.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      expect(installed?.outcome_mode).toBe("create_issue");
    });

    await test.step("the edit dialog carries the same field", async () => {
      await driver.schedulerOpenProjectList(workspaceSlug, projects.first.id);
      await driver.schedulerProjectOpenEdit(handle);
      const state = await driver.schedulerOutcomeState();
      expect(state.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Apply fix"]);
      await driver.schedulerOutcomeSelect("Create issues");
      await driver.schedulerProjectEditSubmit();
      await waitToast(driver, "Install updated");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      const installed = (await serverBindings(workspaceSlug, projects.first.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      expect(installed?.outcome_mode).toBe("create_issue");
    });
  }
);
