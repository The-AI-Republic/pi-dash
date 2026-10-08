// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): the schedulers and prompts routes admit
// workspace admins, members and guests inside the normal workspace chrome,
// refuse outsiders, and never mount a second shell. Row: AGT-022. Both
// route halves are proven in this one scenario.
// Note: outsiders meet the workspace "not found" surface (which mounts no
// sidebar) before the route layouts mount, so the layout-level refusal
// panel is unreachable while that wrapper guard stands; the scenario pins
// the effective refusal on both routes instead.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";
import { outsiderUser, schedulerHarness, seatGuest, seatMember } from "../support";

const ROWS = ["AGT-022"];

test(
  specTitle(ROWS, "schedulers and prompts gates admit roles, refuse outsiders, keep one shell"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    test.setTimeout(600_000);
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt22"));
    const { owner, workspaceSlug } = harness;
    const member = await seatMember(harness);
    const guest = await seatGuest(harness);
    const outsider = await outsiderUser();

    for (const role of [
      { label: "admin", email: owner.email, password: owner.password },
      { label: "member", email: member.email, password: member.password },
      { label: "guest", email: guest.email, password: guest.password },
    ]) {
      await test.step(`${role.label} renders both routes inside one shell`, async () => {
        await driver.resetSession();
        await driver.rulesEnsureSignedIn(role.email, role.password, workspaceSlug);
        await driver.schedulerOpenCatalog(workspaceSlug);
        expect(await driver.schedulerNotAuthorizedVisible()).toBe(false);
        expect(await driver.schedulerWorkspaceNotFoundVisible()).toBe(false);
        expect(await driver.schedulerShellCount()).toBe(1);
        await driver.schedulerOpenPrompts(workspaceSlug);
        expect(await driver.schedulerNotAuthorizedVisible()).toBe(false);
        expect(await driver.schedulerWorkspaceNotFoundVisible()).toBe(false);
        expect(await driver.schedulerShellCount()).toBe(1);
      });
    }

    await test.step("outsider is refused on both routes with no shell at all", async () => {
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(outsider.email, outsider.password);
      await driver.schedulerOpenCatalog(workspaceSlug);
      expect(await driver.schedulerWorkspaceNotFoundVisible()).toBe(true);
      expect(await driver.schedulerShellCount()).toBe(0);
      await driver.schedulerOpenPrompts(workspaceSlug);
      expect(await driver.schedulerWorkspaceNotFoundVisible()).toBe(true);
      expect(await driver.schedulerShellCount()).toBe(0);
    });
  }
);
