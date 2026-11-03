// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the automations settings page admits
// project admins to the two built-in rows with no extension
// contributions below them, and refuses everyone else with the
// not-authorized view. Both rows funnel failures through one handler,
// so either row's failed update toasts the same generic notice. The
// refusal halves run first so every sign-in keeps its retry budget.
// Row: AGT-037.
import { test, expect } from "../../fixtures";
import { ROLE, ensureProject, parityProjectIdentifier } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";
import { toastMessage } from "./support";

const ROWS = ["AGT-037"];

test(
  specTitle(ROWS, "automations page gates on project admin and hosts an empty extension slot"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt37"));
    const { owner, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project", async () =>
      ensureProject(workspaceSlug, harness.ownerSession, `AGT37 Project ${tag}`, parityProjectIdentifier("AG37")));

    await test.step("a project member is refused the page", async () => {
      const member = await seatProjectRole(harness, project.id, ROLE.MEMBER, ROLE.MEMBER, "parity-agt37m");
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(true);
    });

    await test.step("a project guest is refused the page", async () => {
      const guest = await seatProjectRole(harness, project.id, ROLE.GUEST, ROLE.GUEST, "parity-agt37g");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(true);
    });

    await test.step("the owner sees the built-in rows with an empty slot", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(false);
      expect(await driver.automationsBuiltInRows()).toEqual([
        "Auto-archive closed work items",
        "Auto-close work items",
      ]);
      expect(await driver.automationsHasExtensionRows()).toBe(false);
    });

    await test.step("both rows funnel failures through one notice", async () => {
      await driver.automationsFailUpdateOnce();
      await driver.automationsArchiveToggle();
      expect(await toastMessage(driver, "Error!")).toContain("Something went wrong. Please try again.");
      await driver.automationsFailUpdateOnce();
      await driver.automationsCloseToggle();
      expect(await toastMessage(driver, "Error!")).toContain("Something went wrong. Please try again.");
    });
  }
);
