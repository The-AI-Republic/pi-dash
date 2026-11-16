// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): one project's install list shows every
// install with its readable schedule, next/last runs and inline enable
// switch; rows open the install detail while management cells never
// navigate; members and guests get a read-only list; a project with no
// installs invites creation.
// Row: AGT-007.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-007"];

/** Poll the install list until its handles match (the panel has no loader: the empty state doubles as one). */
async function expectRowHandles(
  driver: { schedulerProjectRows: () => Promise<{ handle: string }[]> },
  handles: string[]
): Promise<void> {
  const wanted = [...handles].sort();
  await expect
    .poll(async () => (await driver.schedulerProjectRows()).map((row) => row.handle).sort(), {
      timeout: 30_000,
    })
    .toEqual(wanted);
}

test(
  specTitle(ROWS, "install list shows schedules and runs; management stays admin-only"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt7"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project with two installs", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT7 Project ${tag}`,
        parityProjectIdentifier("AG7")
      );
      const first = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt7-first-${workspaceSlug}`,
        name: "AGT7 First Definition",
        prompt: "Audit this project nightly.",
        color: "#22c55e",
        is_enabled: true,
      });
      const second = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt7-second-${workspaceSlug}`,
        name: "AGT7 Second Definition",
        prompt: "Paused for now.",
        is_enabled: true,
      });
      const dtstart = new Date(Date.now() + 24 * 3600_000).toISOString();
      await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: first.id,
        project: created.id,
        dtstart,
        rrule: "FREQ=DAILY",
      });
      await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: second.id,
        project: created.id,
        dtstart,
        rrule: "FREQ=WEEKLY;BYDAY=MO",
        enabled: false,
      });
      return created;
    });

    await test.step("owner signs in and opens the install list", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
    });

    await test.step("every install renders with schedule, runs, switch and update time", async () => {
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expectRowHandles(
        driver,
        server.map((row) => row.scheduler_slug)
      );
      const rows = await driver.schedulerProjectRows();
      // Display order follows the API order (newest first).
      expect(rows.map((row) => row.handle)).toEqual(server.map((row) => row.scheduler_slug));
      expect(await driver.schedulerProjectEmptyVisible()).toBe(false);
      expect(await driver.schedulerProjectNewVisible()).toBe(true);
      for (const row of rows) {
        const match = server.find((candidate) => candidate.scheduler_slug === row.handle);
        expect(match).toBeDefined();
        expect(row.name).toBe(match?.scheduler_name ?? "");
        expect(row.schedule.length).toBeGreaterThan(0);
        expect(row.scheduleTitle).toBe(match?.rrule ?? "");
        // The seeded stack never fires, so every install awaits its first run.
        expect(row.nextRun.length).toBeGreaterThan(0);
        expect(row.nextRun).not.toBe("(never)");
        expect(row.lastRun).toBe("(never)");
        expect(row.status).toBe((match?.enabled ?? false) ? "Enabled" : "Disabled");
        expect(row.updated.length).toBeGreaterThan(0);
        expect(row.manageVisible).toBe(true);
        const toggle = await driver.schedulerProjectToggleState(row.handle);
        expect(toggle.checked).toBe(match?.enabled ?? false);
        expect(toggle.disabled).toBe(false);
      }
    });

    await test.step("a row opens the install detail", async () => {
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      const target = server[0];
      expect(target).toBeDefined();
      if (target === undefined) throw new Error("[parity] expected at least one install.");
      await driver.schedulerProjectRowOpen(target.scheduler_slug);
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe(target.scheduler_name);
      expect(header.handle).toBe(target.scheduler_slug);
      await driver.schedulerBindingBackToList();
      expect((await driver.schedulerProjectRows()).length).toBe(server.length);
    });

    await test.step("management cells never navigate", async () => {
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      const target = server[0];
      expect(target).toBeDefined();
      if (target === undefined) throw new Error("[parity] expected at least one install.");
      await driver.schedulerProjectOpenEdit(target.scheduler_slug);
      // An edit dialog opened in place: the row click beneath it never
      // navigated to the detail page (which shows no such dialog).
      expect(await driver.schedulerProjectEditOpen()).toBe(true);
      await driver.schedulerCloseProjectEdit();
      expect((await driver.schedulerProjectRows()).length).toBe(server.length);
    });

    await test.step("project member gets the same list read-only", async () => {
      const member = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.MEMBER, "parity-agt7m");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expectRowHandles(
        driver,
        server.map((row) => row.scheduler_slug)
      );
      const rows = await driver.schedulerProjectRows();
      expect(await driver.schedulerProjectNewVisible()).toBe(false);
      for (const row of rows) {
        expect(row.manageVisible).toBe(false);
        const toggle = await driver.schedulerProjectToggleState(row.handle);
        expect(toggle.disabled).toBe(true);
      }
    });

    await test.step("project guest gets the same list read-only", async () => {
      const guest = await seatProjectRole(harness, project.id, ROLE.GUEST, ROLE.GUEST, "parity-agt7g");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      const server = await serverBindings(workspaceSlug, project.id, ownerSession);
      await expectRowHandles(
        driver,
        server.map((row) => row.scheduler_slug)
      );
      const rows = await driver.schedulerProjectRows();
      expect(await driver.schedulerProjectNewVisible()).toBe(false);
      for (const row of rows) {
        expect(row.manageVisible).toBe(false);
        const toggle = await driver.schedulerProjectToggleState(row.handle);
        expect(toggle.disabled).toBe(true);
      }
    });

    await test.step("a project with no installs invites creation", async () => {
      const bare = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT7 Bare ${tag}`,
        parityProjectIdentifier("AG7B")
      );
      expect(await serverBindings(workspaceSlug, bare.id, ownerSession)).toEqual([]);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, bare.id);
      expect(await driver.schedulerProjectEmptyVisible()).toBe(true);
      expect(await driver.schedulerProjectRows()).toEqual([]);
      expect(await driver.schedulerProjectNewVisible()).toBe(true);
    });
  }
);
