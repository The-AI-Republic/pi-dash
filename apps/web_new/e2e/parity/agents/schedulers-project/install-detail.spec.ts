// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): an install's detail page shows its full
// configuration, the composed prompt behind a reveal toggle, and the run
// history with its empty states; a finished finite series shows no
// completion badge yet; run rows and the last-error panel have no live rows
// to render in the seeded stack.
// Row: AGT-013.
// Note: the Completed badge ("Schedule completed — every occurrence has
// run.") is the main#559 target — no series_exhausted signal exists in this
// checkout, so an exhausted finite series renders no badge (Gap). The
// last-error panel and run-row links need live firings, which the seeded
// stack never produces (no beat); the panel is asserted absent and the link
// half is a Gap.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindingDetail,
  serverBindings,
  serverCreateBinding,
  serverPatchBinding,
  serverPatchScheduler,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-013"];

test(
  specTitle(ROWS, "install detail shows config, prompt toggle, states and empty history"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag13"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt13-main-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with one rich install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT13 Project ${tag}`,
        parityProjectIdentifier("AG13")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT13 Main Definition",
        prompt: "Audit this project nightly.",
        color: "#8b5cf6",
        is_enabled: true,
      });
      const now = Date.now();
      const dtstart = new Date(now + 2 * 24 * 3600_000).toISOString();
      const extra = await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        dtstart,
        tzid: "Europe/Paris",
        rrule: "FREQ=WEEKLY;BYDAY=FR",
        rdates: [new Date(now + 3 * 24 * 3600_000).toISOString()],
        exdates: [new Date(now + 10 * 24 * 3600_000).toISOString()],
        extra_context: "AGT13 project framing.",
        outcome_mode: "apply_fix",
      }).catch(() => undefined);
      if (extra === undefined) {
        const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
        if (!rows.some((row) => row.scheduler_slug === handle)) {
          throw new Error("[parity] the AGT13 install neither created nor listed.");
        }
      }
      return created;
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT13 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the install detail", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
    });

    await test.step("the header names the install with its badges and controls", async () => {
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe("AGT13 Main Definition");
      expect(header.handle).toBe(handle);
      expect(header.badges).toContain("builtin");
      expect(header.workspaceLinkVisible).toBe(true);
      expect(header.editVisible).toBe(true);
      expect(header.uninstallVisible).toBe(true);
    });

    await test.step("the config grid renders every stored field", async () => {
      const server = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      const config = await driver.schedulerBindingConfig();
      const valueOf = (label: string): string | undefined => config.find((row) => row.label === label)?.value;
      expect(valueOf("Schedule")).toContain("Friday");
      expect(await driver.schedulerBindingScheduleTitle()).toBe(server.rrule);
      expect(Math.abs(new Date(valueOf("Starts at") ?? 0).getTime() - new Date(server.dtstart).getTime())).toBeLessThan(
        60_000
      );
      expect(valueOf("Time zone")).toBe("Europe/Paris");
      expect(
        Math.abs(new Date(valueOf("Next run") ?? 0).getTime() - new Date(server.next_run_at ?? 0).getTime())
      ).toBeLessThan(60_000);
      expect(valueOf("Outcome mode")).toBe("apply_fix");
      expect(valueOf("Pod")).toBe("(default pod)");
      expect(valueOf("Extra dates")?.length).toBeGreaterThan(0);
      expect(valueOf("Skipped dates")?.length).toBeGreaterThan(0);
      expect(valueOf("Installed by")).toBe(owner.email.split("@")[0]);
      expect(valueOf("Enabled")).toBe("Enabled");
      expect(await driver.schedulerBindingExtraContext()).toBe("AGT13 project framing.");
      expect(await driver.schedulerBindingLastError()).toBeNull();
    });

    await test.step("the composed prompt stays hidden until revealed", async () => {
      const server = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      expect(server.resolved_prompt.length).toBeGreaterThan(0);
      expect(await driver.schedulerBindingPromptState()).toEqual({ toggleVisible: true, revealed: false });
      await driver.schedulerBindingPromptToggle();
      expect(await driver.schedulerBindingPromptState()).toEqual({ toggleVisible: true, revealed: true });
      expect(await driver.schedulerBindingPromptText()).toBe(server.resolved_prompt);
      await driver.schedulerBindingPromptToggle();
      expect(await driver.schedulerBindingPromptState()).toEqual({ toggleVisible: true, revealed: false });
    });

    await test.step("an install awaiting its first run explains the empty history", async () => {
      expect(await driver.schedulerBindingRunsCount()).toBe("0 runs");
      expect(await driver.schedulerBindingRuns()).toEqual([]);
      let empty: string | null = null;
      await expect
        .poll(
          async () => {
            empty = await driver.schedulerBindingRunsEmpty();
            return empty ?? "";
          },
          { timeout: 30_000 }
        )
        .toContain("No runs yet");
      expect(empty ?? "").toContain("next run at");
      expect(await driver.schedulerBindingRunsPager()).toBeNull();
    });

    await test.step("a disabled install names the disabled empty state instead", async () => {
      await serverPatchBinding(workspaceSlug, project.id, await bindingId(), ownerSession, { enabled: false });
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      let empty: string | null = null;
      await expect
        .poll(
          async () => {
            empty = await driver.schedulerBindingRunsEmpty();
            return empty ?? "";
          },
          { timeout: 30_000 }
        )
        .toContain("Scheduler is disabled");
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Enabled")?.value).toBe("Disabled");
      await serverPatchBinding(workspaceSlug, project.id, await bindingId(), ownerSession, { enabled: true });
    });

    await test.step("a disabled definition badges the whole-workspace halt", async () => {
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === handle);
      expect(definition).toBeDefined();
      if (definition === undefined) throw new Error("[parity] expected the AGT13 definition.");
      await serverPatchScheduler(workspaceSlug, definition.id, ownerSession, { is_enabled: false });
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      const header = await driver.schedulerBindingHeader();
      expect(header.badges).toContain("Disabled for the whole workspace");
      await serverPatchScheduler(workspaceSlug, definition.id, ownerSession, { is_enabled: true });
    });

    await test.step("an exhausted finite series shows no completion badge yet", async () => {
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt13-once-${workspaceSlug}`,
        name: "AGT13 Single Definition",
        prompt: "Fires once, in the past.",
        is_enabled: true,
      });
      const past = new Date(Date.now() - 2 * 24 * 3600_000).toISOString();
      const rows = await serverBindings(workspaceSlug, project.id, ownerSession);
      let onceId = rows.find((row) => row.scheduler === definition.id)?.id;
      if (onceId === undefined) {
        onceId = (
          await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
            scheduler: definition.id,
            project: project.id,
            dtstart: past,
            rrule: "",
          })
        ).id;
      }
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, onceId);
      const header = await driver.schedulerBindingHeader();
      expect(header.badges).not.toContain("Completed");
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Schedule")?.value).toContain("Once at dtstart");
    });

    await test.step("a project member reads the detail without managing it", async () => {
      const member = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.MEMBER, "parity-ag13m");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe("AGT13 Main Definition");
      expect(header.workspaceLinkVisible).toBe(false);
      expect(header.editVisible).toBe(false);
      expect(header.uninstallVisible).toBe(false);
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Schedule")?.value).toContain("Friday");
      const toggle = await driver.schedulerBindingToggleState();
      expect(toggle.disabled).toBe(true);
      expect(await driver.schedulerBindingRunsCount()).toBe("0 runs");
    });
  }
);
