// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): flipping an install off and on from the
// list applies immediately and persists; the switch gates input mid-flight
// so a double submit cannot race; a rejected flip rolls back with an error
// notice; firing stops while the install is off (its occurrences vanish
// from the calendar); the detail-page switch flips the same install.
// Row: AGT-011.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  addProjectMembers,
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverOccurrences,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-011"];

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

/** Poll one install's server-side enabled flag until it reads `want`. */
async function expectServerEnabled(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  session: string,
  want: boolean
): Promise<void> {
  await expect
    .poll(
      async () =>
        (await serverBindings(workspaceSlug, projectId, session)).find((row) => row.id === bindingId)?.enabled,
      { timeout: 30_000 }
    )
    .toBe(want);
}

test(
  specTitle(ROWS, "toggle flips immediately, gates double submits, rolls back, stops firing"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag11"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt11-main-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT11 Project ${tag}`,
        parityProjectIdentifier("AG11")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT11 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        // Anchored in the past so every visible week holds future firings.
        dtstart: new Date(Date.now() - 30 * 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return created;
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT11 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the install list", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
    });

    await test.step("flipping off applies immediately and persists", async () => {
      await driver.schedulerProjectToggle(handle);
      // Optimistic: the switch reports off before the round-trip lands.
      await expect
        .poll(() => driver.schedulerProjectToggleState(handle), { timeout: 30_000 })
        .toEqual({ checked: false, disabled: false });
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, false);
      const message = await toastMessage(driver, "Install updated");
      expect(message).toContain("will not fire until re-enabled");
    });

    await test.step("the switch gates input mid-flight, then flips on", async () => {
      expect(await driver.schedulerProjectToggleFlightGated(handle)).toBe(true);
      await expect
        .poll(() => driver.schedulerProjectToggleState(handle), { timeout: 30_000 })
        .toEqual({ checked: true, disabled: false });
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, true);
      const message = await toastMessage(driver, "Install updated");
      expect(message).toContain("will fire on the next scheduled tick");
    });

    await test.step("a rejected flip rolls back with an error notice", async () => {
      const admin = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.ADMIN, "parity-ag11a");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(admin.email, admin.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await expect
        .poll(() => driver.schedulerProjectToggleState(handle), { timeout: 30_000 })
        .toEqual({ checked: true, disabled: false });
      // Demote below the mutate line after the page loaded: the stale UI
      // still offers the switch, but the server refuses the flip.
      await addProjectMembers(workspaceSlug, project.id, ownerSession, [
        { member_id: admin.userId, role: ROLE.MEMBER },
      ]);
      await driver.schedulerProjectToggle(handle);
      await toastMessage(driver, "Something went wrong");
      await expect
        .poll(() => driver.schedulerProjectToggleState(handle), { timeout: 30_000 })
        .toEqual({ checked: true, disabled: false });
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, true);
      // Restore the seat so later halves run as an admin again.
      await addProjectMembers(workspaceSlug, project.id, ownerSession, [{ member_id: admin.userId, role: ROLE.ADMIN }]);
    });

    await test.step("firing stops while off: occurrences and blocks vanish", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerProjectToggle(handle);
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, false);
      const now = Date.now();
      const from = new Date(now - 7 * 24 * 3600_000).toISOString();
      const to = new Date(now + 7 * 24 * 3600_000).toISOString();
      const bid = await bindingId();
      expect(
        (await serverOccurrences(workspaceSlug, project.id, ownerSession, from, to)).occurrences.filter(
          (row) => row.binding_id === bid
        )
      ).toEqual([]);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, project.id);
      // Step into next week: a fully future window, whatever today is.
      await driver.schedulerCalendarStep("next");
      const blocks = await driver.schedulerCalendarWeekBlocks();
      expect(blocks.filter((row) => row.name === "AGT11 Main Definition")).toEqual([]);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerProjectToggle(handle);
      await expectServerEnabled(workspaceSlug, project.id, bid, ownerSession, true);
      expect(
        (await serverOccurrences(workspaceSlug, project.id, ownerSession, from, to)).occurrences.filter(
          (row) => row.binding_id === bid
        ).length
      ).toBeGreaterThan(0);
    });

    await test.step("the detail-page switch flips the same install", async () => {
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      await expect
        .poll(() => driver.schedulerBindingToggleState(), { timeout: 30_000 })
        .toEqual({
          checked: true,
          disabled: false,
        });
      await driver.schedulerBindingToggle();
      await expect
        .poll(() => driver.schedulerBindingToggleState(), { timeout: 30_000 })
        .toEqual({
          checked: false,
          disabled: false,
        });
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, false);
      await driver.schedulerBindingToggle();
      await expect
        .poll(() => driver.schedulerBindingToggleState(), { timeout: 30_000 })
        .toEqual({
          checked: true,
          disabled: false,
        });
      await expectServerEnabled(workspaceSlug, project.id, await bindingId(), ownerSession, true);
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe("AGT11 Main Definition");
    });
  }
);
