// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): installing a catalog definition on one
// project offers only enabled, not-yet-bound definitions; submit requires a
// definition and a start; backend field errors surface without closing the
// modal; a project admin who cannot author definitions meets a dead end when
// nothing is installable.
// Row: AGT-008.
// Note: the row's "viewers can browse the dialog" half is unreachable — the
// New control is project-admin-only, so the modal has no non-admin entry
// point (viewers get the read-only list proven in AGT-007).
// Note: the modal picks its initial path from data that may still be loading
// when it mounts, so a cold open can land on either path; the scenario
// navigates to the needed path and only asserts the initial path once the
// project's data has settled in the UI.
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

const ROWS = ["AGT-008"];

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

/** `datetime-local` input value ("YYYY-MM-DDTHH:mm") for a moment days out at 09:30 local. */
function localInputDaysOut(days: number): string {
  const at = new Date();
  at.setDate(at.getDate() + days);
  at.setHours(9, 30, 0, 0);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(
    at.getMinutes()
  )}`;
}

test(
  specTitle(ROWS, "install offers only eligible definitions; failures stay open, success refreshes"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt8"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT8 Project ${tag}`,
        parityProjectIdentifier("AG8")
      );
      const bound = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt8-bound-${workspaceSlug}`,
        name: "AGT8 Bound Definition",
        prompt: "Already installed here.",
        is_enabled: true,
      });
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt8-free-${workspaceSlug}`,
        name: "AGT8 Free Definition",
        prompt: "Available to install.",
        is_enabled: true,
      });
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt8-off-${workspaceSlug}`,
        name: "AGT8 Disabled Definition",
        prompt: "Disabled at the workspace level.",
        is_enabled: false,
      });
      await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: bound.id,
        project: created.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return created;
    });

    await test.step("owner signs in and opens the install path", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerOpenProjectInstall();
      // The modal picks its initial path from data that may still be
      // loading on a fresh page, so navigate to the install path instead
      // of asserting which path it opened on.
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
      expect(await driver.schedulerProjectInstallTabs()).toEqual(["Install existing", "Create new"]);
    });

    await test.step("only enabled, not-yet-bound definitions are offered", async () => {
      const server = await serverSchedulers(workspaceSlug, ownerSession);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      const boundIds = new Set(bindings.map((row) => row.scheduler));
      const eligible = server
        .filter((row) => row.is_enabled && !boundIds.has(row.id))
        .map((row) => row.slug)
        .sort();
      expect(eligible).toContain(`agt8-free-${workspaceSlug}`);
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle).sort(), {
          timeout: 30_000,
        })
        .toEqual(eligible);
      const options = await driver.schedulerProjectInstallOptions();
      expect(options.map((row) => row.handle)).not.toContain(`agt8-bound-${workspaceSlug}`);
      expect(options.map((row) => row.handle)).not.toContain(`agt8-off-${workspaceSlug}`);
    });

    await test.step("submit requires a start", async () => {
      await driver.schedulerProjectInstallSelect(`agt8-free-${workspaceSlug}`);
      await driver.schedulerProjectInstallFillSchedule({ dtstart: "" });
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
        .toContain("Start time is required.");
      expect(await driver.schedulerProjectInstallOpen()).toBe(true);
      expect(
        (await serverBindings(workspaceSlug, project.id, ownerSession)).some(
          (row) => row.scheduler_slug === `agt8-free-${workspaceSlug}`
        )
      ).toBe(false);
    });

    await test.step("backend field errors surface without closing", async () => {
      await driver.schedulerProjectInstallFillSchedule({
        dtstart: localInputDaysOut(3),
        rrule: "FREQ=NOPE",
      });
      await driver.schedulerProjectInstallSubmit();
      const message = await toastMessage(driver, "Something went wrong");
      expect(message).toContain("FREQ");
      expect(await driver.schedulerProjectInstallOpen()).toBe(true);
      expect(
        (await serverBindings(workspaceSlug, project.id, ownerSession)).some(
          (row) => row.scheduler_slug === `agt8-free-${workspaceSlug}`
        )
      ).toBe(false);
    });

    await test.step("a valid submit installs, closes and refreshes the list", async () => {
      await driver.schedulerProjectInstallFillSchedule({
        dtstart: localInputDaysOut(3),
        tzid: "America/New_York",
        rrule: "FREQ=WEEKLY;BYDAY=TU",
        extraContext: "AGT8 project framing.",
      });
      await driver.schedulerProjectInstallSubmit();
      const message = await toastMessage(driver, "Scheduler installed");
      expect(message).toContain("configured schedule");
      await expect.poll(() => driver.schedulerProjectInstallOpen(), { timeout: 30_000 }).toBe(false);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      const installed = bindings.find((row) => row.scheduler_slug === `agt8-free-${workspaceSlug}`);
      expect(installed).toBeDefined();
      expect(installed?.tzid).toBe("America/New_York");
      expect(installed?.rrule).toBe("FREQ=WEEKLY;BYDAY=TU");
      expect(installed?.extra_context).toBe("AGT8 project framing.");
      expect(installed?.enabled).toBe(true);
      expect(installed?.outcome_mode).toBe("create_issue");
      const skew = Math.abs(new Date(installed?.dtstart ?? 0).getTime() - new Date(localInputDaysOut(3)).getTime());
      expect(skew).toBeLessThan(60_000);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle), { timeout: 30_000 })
        .toContain(`agt8-free-${workspaceSlug}`);
    });

    const fullProject = await test.step("owner prepares a fully installed project", async () =>
      ensureProject(workspaceSlug, ownerSession, `AGT8 Full ${tag}`, parityProjectIdentifier("AG8F")));

    await test.step("submit without a definition is refused on a fully installed project", async () => {
      const full = fullProject;
      for (const row of await serverSchedulers(workspaceSlug, ownerSession)) {
        if (!row.is_enabled) continue;
        await serverCreateBinding(workspaceSlug, full.id, ownerSession, {
          scheduler: row.id,
          project: full.id,
          dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        }).catch(() => undefined);
      }
      await driver.schedulerOpenProjectList(workspaceSlug, full.id);
      // Settle the project's bindings first: the modal reads them at open
      // time to pick its initial path.
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual((await serverBindings(workspaceSlug, full.id, ownerSession)).map((row) => row.scheduler_slug).sort());
      await driver.schedulerOpenProjectInstall();
      // Nothing installable, so the modal opens on the create path; the
      // install path behind its tab offers no definition to submit.
      expect(await driver.schedulerProjectInstallMode()).toBe("create");
      await driver.schedulerProjectInstallSelectTab("Install existing");
      expect(await driver.schedulerProjectInstallOptions()).toEqual([]);
      expect(await driver.schedulerProjectInstallSubmitDisabled()).toBe(true);
      await driver.schedulerCloseProjectInstall();
    });

    await test.step("a project admin who cannot author meets the dead end there", async () => {
      const full = fullProject;
      const admin = await seatProjectRole(harness, full.id, ROLE.MEMBER, ROLE.ADMIN, "parity-agt8a");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(admin.email, admin.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, full.id);
      expect(await driver.schedulerProjectNewVisible()).toBe(true);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), { timeout: 30_000 })
        .toEqual((await serverBindings(workspaceSlug, full.id, ownerSession)).map((row) => row.scheduler_slug).sort());
      await driver.schedulerOpenProjectInstall();
      expect(await driver.schedulerProjectInstallMode()).toBe("dead-end");
      const text = await driver.schedulerProjectInstallDeadEndText();
      expect(text).toContain("No schedulers available");
      expect(text).toContain("workspace admin");
      await driver.schedulerCloseProjectInstall();
    });
  }
);
