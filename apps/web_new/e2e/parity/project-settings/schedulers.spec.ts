// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-393): scheduler installs through the project
// settings tab. The settings schedulers tab lists installs with a working
// enabled switch and deep-links rows to install detail; eligible
// workspace schedulers install through the two-path dialog, which also
// authors new definitions in one submit for workspace admins and
// dead-ends when nothing is installable and the viewer cannot author;
// install configuration (start, timezone, recurrence with live preview,
// appended context, enabled flag, findings handling, runner-pool
// override) round-trips; the detail page shows config, the composed
// prompt and run history with a not-available state for missing
// installs; edits persist and uninstalling keeps the definition.
//
// Every scenario builds a fresh harness workspace, so installs and
// definitions never leak across tests; each test still deletes the
// installs and definitions it created. The dialog, table and detail
// drivers are shared with the agents-area scenarios; only the entry
// point here is the settings tab.
// Rows: PSET-055–061.
import { test, expect } from "../fixtures";
import {
  ROLE,
  addProjectMembers,
  createPod,
  deletePod,
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindingDetail,
  serverBindings,
  serverCleanupBinding,
  serverCleanupScheduler,
  serverCreateBinding,
  serverSchedulers,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../agents/support";

/** `datetime-local` input value ("YYYY-MM-DDTHH:mm") for a moment days out at 09:30 local. */
function localStartDaysOut(days: number): string {
  const at = new Date();
  at.setDate(at.getDate() + days);
  at.setHours(9, 30, 0, 0);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(
    at.getMinutes()
  )}`;
}

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
  specTitle(["PSET-055"], "settings table lists installs, flips enablement, and deep-links rows"),
  { tag: specTags(["PSET-055"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3955"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const firstHandle = `ps55-first-${workspaceSlug}`;
    const secondHandle = `ps55-second-${workspaceSlug}`;
    const createdIds: { binding?: string; scheduler?: string }[] = [];
    const project = await test.step("owner prepares a project with two installs", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS55 Project ${tag}`,
        parityProjectIdentifier("PS55")
      );
      const first = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: firstHandle,
        name: "PS55 First Definition",
        prompt: "Audit this project nightly.",
        color: "#22c55e",
        is_enabled: true,
      });
      const second = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: secondHandle,
        name: "PS55 Second Definition",
        prompt: "Paused for now.",
        is_enabled: true,
      });
      const dtstart = new Date(Date.now() + 24 * 3600_000).toISOString();
      const firstBinding = await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: first.id,
        project: created.id,
        dtstart,
        rrule: "FREQ=DAILY",
      });
      const secondBinding = await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: second.id,
        project: created.id,
        dtstart,
        rrule: "FREQ=WEEKLY;BYDAY=MO",
        enabled: false,
      });
      createdIds.push({ binding: firstBinding.id, scheduler: first.id });
      createdIds.push({ binding: secondBinding.id, scheduler: second.id });
      return created;
    });
    try {
      await test.step("owner signs in and opens the settings schedulers tab", async () => {
        await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
        await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
        expect(await driver.schedulerSettingsPanelVisible()).toBe(true);
      });

      await test.step("every install renders with its schedule, runs and switch", async () => {
        const server = await serverBindings(workspaceSlug, project.id, ownerSession);
        await expect
          .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), {
            timeout: 30_000,
          })
          .toEqual(server.map((row) => row.scheduler_slug).sort());
        const rows = await driver.schedulerProjectRows();
        for (const row of rows) {
          const match = server.find((candidate) => candidate.scheduler_slug === row.handle);
          expect(match).toBeDefined();
          expect(row.name).toBe(match?.scheduler_name ?? "");
          expect(row.schedule.length).toBeGreaterThan(0);
          expect(row.scheduleTitle).toBe(match?.rrule ?? "");
          expect(row.nextRun.length).toBeGreaterThan(0);
          expect(row.nextRun).not.toBe("(never)");
          expect(row.lastRun).toBe("(never)");
          expect(row.status).toBe((match?.enabled ?? false) ? "Enabled" : "Disabled");
          expect(row.updated.length).toBeGreaterThan(0);
          const toggle = await driver.schedulerProjectToggleState(row.handle);
          expect(toggle.checked).toBe(match?.enabled ?? false);
          expect(toggle.disabled).toBe(false);
        }
      });

      await test.step("the switch persists its flip", async () => {
        await driver.schedulerProjectToggle(secondHandle);
        await expect
          .poll(
            async () =>
              (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
                (row) => row.scheduler_slug === secondHandle
              )?.enabled,
            { timeout: 30_000 }
          )
          .toBe(true);
        await driver.schedulerProjectToggle(secondHandle);
        await expect
          .poll(
            async () =>
              (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
                (row) => row.scheduler_slug === secondHandle
              )?.enabled,
            { timeout: 30_000 }
          )
          .toBe(false);
      });

      await test.step("a rejected flip rolls back with an error notice", async () => {
        const admin = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.ADMIN, "parity-3955a");
        await driver.resetSession();
        await driver.rulesEnsureSignedIn(admin.email, admin.password, workspaceSlug);
        await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
        await expect
          .poll(() => driver.schedulerProjectToggleState(firstHandle), { timeout: 30_000 })
          .toEqual({ checked: true, disabled: false });
        // Demote below the mutate line after the tab loaded: the stale UI
        // still offers the switch, but the server refuses the flip.
        await addProjectMembers(workspaceSlug, project.id, ownerSession, [
          { member_id: admin.userId, role: ROLE.MEMBER },
        ]);
        await driver.schedulerProjectToggle(firstHandle);
        await toastMessage(driver, "Something went wrong");
        await expect
          .poll(() => driver.schedulerProjectToggleState(firstHandle), { timeout: 30_000 })
          .toEqual({ checked: true, disabled: false });
      });

      await test.step("a row opens the install detail", async () => {
        await driver.resetSession();
        await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
        await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
        await driver.schedulerProjectRowOpen(firstHandle);
        const header = await driver.schedulerBindingHeader();
        expect(header.handle).toBe(firstHandle);
        expect(header.name).toBe("PS55 First Definition");
      });
    } finally {
      for (const created of createdIds) {
        if (created.binding) await serverCleanupBinding(workspaceSlug, project.id, created.binding, ownerSession);
        if (created.scheduler) await serverCleanupScheduler(workspaceSlug, created.scheduler, ownerSession);
      }
    }
  }
);

test(
  specTitle(["PSET-056", "PSET-057"], "eligible schedulers install here; viewers who cannot author dead-end"),
  { tag: specTags(["PSET-056", "PSET-057"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3956"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `ps56-main-${workspaceSlug}`;
    const project = await test.step("owner prepares a project and a definition", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS56 Project ${tag}`,
        parityProjectIdentifier("PS56")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "PS56 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      return created;
    });
    let installedBinding: string | undefined;
    try {
      await test.step("owner installs the eligible scheduler from the settings tab", async () => {
        await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
        await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
        expect(await driver.schedulerProjectEmptyVisible()).toBe(true);
        await driver.schedulerOpenProjectInstall();
        // The modal picks its initial path from data that may still be
        // loading on a fresh page, so navigate to the install path
        // instead of asserting which path it opened on.
        if ((await driver.schedulerProjectInstallMode()) !== "install") {
          await driver.schedulerProjectInstallSelectTab("Install existing");
        }
        expect(await driver.schedulerProjectInstallMode()).toBe("install");
        await expect
          .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), {
            timeout: 30_000,
          })
          .toContain(handle);
        await driver.schedulerProjectInstallSelect(handle);
        await driver.schedulerProjectInstallFillSchedule({
          dtstart: localStartDaysOut(2),
          rrule: "FREQ=DAILY",
        });
        expect(await driver.schedulerProjectInstallHumanizer()).not.toBe("");
        await driver.schedulerProjectInstallSubmit();
        await toastMessage(driver, "Scheduler installed");
        const installed = await expect
          .poll(
            async () =>
              (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
                (row) => row.scheduler_slug === handle
              ),
            { timeout: 30_000 }
          )
          .not.toBeUndefined();
        void installed;
        installedBinding = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
          (row) => row.scheduler_slug === handle
        )?.id;
        expect(installedBinding).toBeTruthy();
      });

      await test.step("a project admin who cannot author dead-ends when nothing is installable", async () => {
        const fullHarness = await schedulerHarness("parity-3956e");
        const fullProject = await ensureProject(
          fullHarness.workspaceSlug,
          fullHarness.ownerSession,
          `PS56 Full ${fullHarness.tag}`,
          parityProjectIdentifier("PS56")
        );
        // Fresh workspaces are born with a seeded catalog, so bind every
        // enabled definition: with nothing left installable, a viewer
        // who cannot author meets the dead end.
        for (const row of await serverSchedulers(fullHarness.workspaceSlug, fullHarness.ownerSession)) {
          if (!row.is_enabled) continue;
          await serverCreateBinding(fullHarness.workspaceSlug, fullProject.id, fullHarness.ownerSession, {
            scheduler: row.id,
            project: fullProject.id,
            dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
            rrule: "FREQ=DAILY",
          }).catch(() => undefined);
        }
        // Workspace member plus project admin: inside the settings tab but
        // without the workspace-admin author path.
        const viewer = await seatProjectRole(fullHarness, fullProject.id, ROLE.MEMBER, ROLE.ADMIN, "parity-3956v");
        await driver.resetSession();
        await driver.rulesEnsureSignedIn(viewer.email, viewer.password, fullHarness.workspaceSlug);
        await driver.schedulerOpenSettingsSchedulers(fullHarness.workspaceSlug, fullProject.id);
        // Settle the project's rows first: the modal reads them at open
        // time to pick its initial path.
        await expect
          .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), {
            timeout: 30_000,
          })
          .toEqual(
            (await serverBindings(fullHarness.workspaceSlug, fullProject.id, fullHarness.ownerSession))
              .map((row) => row.scheduler_slug)
              .sort()
          );
        await driver.schedulerOpenProjectInstall();
        await expect.poll(() => driver.schedulerProjectInstallMode(), { timeout: 30_000 }).toBe("dead-end");
        const deadEnd = await driver.schedulerProjectInstallDeadEndText();
        expect(deadEnd).toContain("No schedulers available");
        expect(deadEnd).toContain("workspace admin");
        expect(await driver.schedulerProjectInstallTabs()).not.toContain("Create new");
        await driver.schedulerCloseProjectInstall();
        for (const row of await serverBindings(fullHarness.workspaceSlug, fullProject.id, fullHarness.ownerSession)) {
          await serverCleanupBinding(fullHarness.workspaceSlug, fullProject.id, row.id, fullHarness.ownerSession);
        }
      });
    } finally {
      if (installedBinding) await serverCleanupBinding(workspaceSlug, project.id, installedBinding, ownerSession);
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === handle);
      if (definition) await serverCleanupScheduler(workspaceSlug, definition.id, ownerSession);
    }
  }
);

test(
  specTitle(["PSET-057"], "authors create a definition and install it in one submit"),
  { tag: specTags(["PSET-057"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3957"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const slug = `ps57-authored-${workspaceSlug}`;
    const project = await test.step("owner prepares a project and a taken slug", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS57 Project ${tag}`,
        parityProjectIdentifier("PS57")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug,
        name: "PS57 Taken Definition",
        prompt: "Already here.",
        is_enabled: true,
      });
      return created;
    });
    let installedBinding: string | undefined;
    try {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
      await driver.schedulerOpenProjectInstall();
      await driver.schedulerProjectInstallSelectTab("Create new");

      await test.step("the handle derives from the name", async () => {
        await driver.schedulerProjectCreateFillName("PS57 Authored Definition");
        await expect.poll(() => driver.schedulerProjectCreateHandleValue(), { timeout: 30_000 }).not.toBe("");
      });

      await test.step("duplicate slugs flag inline", async () => {
        await driver.schedulerProjectCreateFillHandle(slug);
        await driver.schedulerProjectCreateFillPrompt("This handle is taken.");
        await driver.schedulerProjectInstallSubmit();
        // The conflict maps inline only after the submit round-trip.
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
      });

      await test.step("a valid submit creates the definition and the install", async () => {
        const fresh = `ps57-fresh-${workspaceSlug}`;
        await driver.schedulerProjectCreateFillHandle(fresh);
        await driver.schedulerProjectCreateFillDescription("Authored from the settings tab.");
        await driver.schedulerProjectCreateFillPrompt("Review open work items weekly.");
        await driver.schedulerProjectInstallFillSchedule({
          dtstart: localStartDaysOut(3),
          rrule: "FREQ=WEEKLY;BYDAY=MO",
        });
        await driver.schedulerProjectInstallSubmit();
        await toastMessage(driver, "Scheduler created and installed");
        expect((await serverSchedulers(workspaceSlug, ownerSession)).some((row) => row.slug === fresh)).toBe(true);
        const installed = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
          (row) => row.scheduler_slug === fresh
        );
        expect(installed).toBeDefined();
        installedBinding = installed?.id;
      });
    } finally {
      if (installedBinding) await serverCleanupBinding(workspaceSlug, project.id, installedBinding, ownerSession);
      for (const doomed of [`ps57-fresh-${workspaceSlug}`, slug]) {
        const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === doomed);
        if (definition) await serverCleanupScheduler(workspaceSlug, definition.id, ownerSession);
      }
    }
  }
);

test(
  specTitle(["PSET-058"], "install configuration round-trips every field"),
  { tag: specTags(["PSET-058"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3958"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `ps58-main-${workspaceSlug}`;
    const project = await test.step("owner prepares a project, a pod and a definition", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS58 Project ${tag}`,
        parityProjectIdentifier("PS58")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "PS58 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      return created;
    });
    const podName = `ps58rig${tag.slice(0, 8)}`;
    const pod = await test.step("owner adds a project pod", async () => createPod(project.id, ownerSession, podName));
    let installedBinding: string | undefined;
    try {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);
      await driver.schedulerOpenProjectInstall();
      if ((await driver.schedulerProjectInstallMode()) !== "install") {
        await driver.schedulerProjectInstallSelectTab("Install existing");
      }
      await expect
        .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), {
          timeout: 30_000,
        })
        .toContain(handle);
      await driver.schedulerProjectInstallSelect(handle);

      await test.step("the recurrence preview tracks the widget", async () => {
        await driver.schedulerProjectInstallFillSchedule({ rrule: "FREQ=WEEKLY;BYDAY=MO" });
        expect(await driver.schedulerProjectInstallHumanizer()).not.toBe("");
      });

      await test.step("the findings field offers its three modes", async () => {
        const state = await driver.schedulerOutcomeState();
        expect(state.options.map((row) => row.label)).toEqual(["Create issues", "Apply fix", "Fix & open for review"]);
        expect(state.options.filter((row) => row.checked).map((row) => row.label)).toEqual(["Create issues"]);
        await driver.schedulerOutcomeSelect("Apply fix");
        expect(
          (await driver.schedulerOutcomeState()).options.filter((row) => row.checked).map((row) => row.label)
        ).toEqual(["Apply fix"]);
      });

      await test.step("a full configuration persists and reads back", async () => {
        // The zone picker lists Intl.supportedValuesOf("timeZone"),
        // which has no UTC entry in modern runtimes (UTC survives only
        // in the no-Intl fallback list), so prove the platform list with
        // a real zone like the agents-area specs do.
        await driver.schedulerProjectInstallFillSchedule({
          dtstart: localStartDaysOut(4),
          tzid: "America/New_York",
          rrule: "FREQ=WEEKLY;BYDAY=MO",
          extraContext: "PSET-058 appended context.",
        });
        const values = await driver.schedulerProjectInstallScheduleValues();
        expect(values.rrule).toContain("FREQ=WEEKLY");
        await expect
          .poll(async () => (await driver.schedulerPodOptions()).map((row) => row.value), { timeout: 30_000 })
          .toContain(pod.id);
        await driver.schedulerPodSelect(pod.id);
        await driver.schedulerProjectInstallSubmit();
        await toastMessage(driver, "Scheduler installed");
        const installed = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
          (row) => row.scheduler_slug === handle
        );
        expect(installed).toBeDefined();
        installedBinding = installed?.id;
        expect(installed?.outcome_mode).toBe("apply_fix");
        const detail = await serverBindingDetail(workspaceSlug, project.id, installedBinding ?? "", ownerSession);
        expect(detail.rrule).toContain("FREQ=WEEKLY");
        expect(detail.tzid).toBe("America/New_York");
        expect(detail.extra_context).toContain("PSET-058 appended context.");
        expect(detail.pod).toBe(pod.id);
        expect(detail.pod_name).toBe(pod.name);
      });
    } finally {
      if (installedBinding) await serverCleanupBinding(workspaceSlug, project.id, installedBinding, ownerSession);
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === handle);
      if (definition) await serverCleanupScheduler(workspaceSlug, definition.id, ownerSession);
      await deletePod(pod.id, ownerSession).catch(() => undefined);
    }
  }
);

test(
  specTitle(["PSET-059"], "install detail shows config, prompt and runs; missing installs explain"),
  { tag: specTags(["PSET-059"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3959"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `ps59-main-${workspaceSlug}`;
    const project = await test.step("owner prepares a project with an install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS59 Project ${tag}`,
        parityProjectIdentifier("PS59")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "PS59 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const binding = await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
        extra_context: "PSET-059 appended context.",
      });
      return { ...created, bindingId: binding.id, schedulerId: definition.id };
    });
    try {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, project.bindingId);

      await test.step("the detail shows config and the composed prompt", async () => {
        const header = await driver.schedulerBindingHeader();
        expect(header.handle).toBe(handle);
        expect(await driver.schedulerBindingConfig()).not.toEqual([]);
        expect(await driver.schedulerBindingLastError()).toBeNull();
        const prompt = await driver.schedulerBindingPromptState();
        expect(prompt.toggleVisible).toBe(true);
        await driver.schedulerBindingPromptToggle();
        expect(await driver.schedulerBindingPromptText()).toContain("Audit this project nightly.");
        expect(await driver.schedulerBindingExtraContext()).toContain("PSET-059 appended context.");
      });

      await test.step("the run history is empty and polls", async () => {
        expect(await driver.schedulerBindingRuns()).toEqual([]);
        expect(await driver.schedulerBindingRunsEmpty()).not.toBeNull();
        await driver.schedulerBindingWaitRunsRefetch();
        expect(await driver.schedulerBindingRuns()).toEqual([]);
      });

      await test.step("a missing install shows the not-available state with a back link", async () => {
        await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, "00000000-0000-0000-0000-000000000000");
        expect(await driver.schedulerBindingRemovedVisible()).toBe(true);
        await driver.schedulerBindingBackToList();
        await expect.poll(async () => (await driver.schedulerProjectRows()).length, { timeout: 30_000 }).toBe(1);
      });
    } finally {
      await serverCleanupBinding(workspaceSlug, project.id, project.bindingId, ownerSession);
      await serverCleanupScheduler(workspaceSlug, project.schedulerId, ownerSession);
    }
  }
);

test(
  specTitle(["PSET-060"], "edits persist and uninstalling keeps the definition"),
  { tag: specTags(["PSET-060"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3960"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `ps60-main-${workspaceSlug}`;
    const project = await test.step("owner prepares a project with an install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS60 Project ${tag}`,
        parityProjectIdentifier("PS60")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "PS60 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const binding = await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return { ...created, bindingId: binding.id, schedulerId: definition.id };
    });
    try {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);

      await test.step("the edit dialog opens prefilled and persists changes", async () => {
        await driver.schedulerProjectOpenEdit(handle);
        expect(await driver.schedulerProjectEditOpen()).toBe(true);
        expect((await driver.schedulerProjectEditValues()).rrule).toContain("FREQ=DAILY");
        await driver.schedulerProjectEditFill({ rrule: "FREQ=WEEKLY;BYDAY=FR" });
        expect(await driver.schedulerProjectEditHumanizer()).not.toBe("");
        await driver.schedulerProjectEditSubmit();
        await toastMessage(driver, "Install updated");
        await expect
          .poll(
            async () =>
              (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
                (row) => row.scheduler_slug === handle
              )?.rrule,
            { timeout: 30_000 }
          )
          .toContain("FREQ=WEEKLY");
      });

      await test.step("uninstalling removes the binding but keeps the definition", async () => {
        await driver.schedulerProjectOpenUninstall(handle);
        expect(await driver.schedulerUninstallDialogText()).toContain("unaffected");
        await driver.schedulerConfirmUninstall();
        await expect
          .poll(async () => serverBindings(workspaceSlug, project.id, ownerSession), { timeout: 30_000 })
          .toEqual([]);
        expect((await serverSchedulers(workspaceSlug, ownerSession)).some((row) => row.slug === handle)).toBe(true);
        project.bindingId = "";
      });
    } finally {
      if (project.bindingId !== "") {
        await serverCleanupBinding(workspaceSlug, project.id, project.bindingId, ownerSession);
      }
      await serverCleanupScheduler(workspaceSlug, project.schedulerId, ownerSession);
    }
  }
);

test(
  specTitle(["PSET-061"], "schedulers show the empty prompt and field-level messages"),
  { tag: specTags(["PSET-061"]) },
  async ({ driver, seed }) => {
    void seed;
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-3961"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `ps61-main-${workspaceSlug}`;
    const project = await test.step("owner prepares an empty project and a definition", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `PS61 Project ${tag}`,
        parityProjectIdentifier("PS61")
      );
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "PS61 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      return created;
    });
    try {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenSettingsSchedulers(workspaceSlug, project.id);

      await test.step("an empty tab invites creation", async () => {
        expect(await driver.schedulerProjectEmptyVisible()).toBe(true);
        expect(await driver.schedulerProjectNewVisible()).toBe(true);
      });

      await test.step("submitting without a start flags the field", async () => {
        await driver.schedulerOpenProjectInstall();
        if ((await driver.schedulerProjectInstallMode()) !== "install") {
          await driver.schedulerProjectInstallSelectTab("Install existing");
        }
        await expect
          .poll(async () => (await driver.schedulerProjectInstallOptions()).map((row) => row.handle), {
            timeout: 30_000,
          })
          .toContain(handle);
        await driver.schedulerProjectInstallSelect(handle);
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
      });

      await test.step("duplicate slugs flag inline on the author path", async () => {
        await driver.schedulerProjectInstallSelectTab("Create new");
        await driver.schedulerProjectCreateFillName("PS61 Duplicate Definition");
        await driver.schedulerProjectCreateFillHandle(handle);
        await driver.schedulerProjectCreateFillPrompt("This handle is taken.");
        // The schedule section is shared across paths: refill the start
        // the previous step cleared so client validation passes and the
        // submit reaches the server-side slug check.
        await driver.schedulerProjectInstallFillSchedule({ dtstart: localStartDaysOut(2) });
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
      });
    } finally {
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === handle);
      if (definition) await serverCleanupScheduler(workspaceSlug, definition.id, ownerSession);
    }
  }
);
