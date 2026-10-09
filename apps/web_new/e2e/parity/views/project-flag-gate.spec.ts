// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the disabled-feature gate on the project
// views list — the explanatory empty state and its admin-only shortcut
// into project settings. Row: VIEW-002. Green on apps/web first.
import { test, expect } from "../fixtures";
import { ROLE, browserSessionCookies, serverProjectViewFlags, setProjectViewFlags } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-002"], "disabled views feature gates the list with a settings shortcut"),
  { tag: specTags(["VIEW-002"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v2", false);
    const { owner, workspaceSlug, projectId } = harness;

    await test.step("a fresh project gates the list with an admin-enabled shortcut", async () => {
      const flags = await serverProjectViewFlags(workspaceSlug, projectId, owner.cookie);
      expect(flags.issue_views_view).toBe(false);
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsGateTitle(), POLL).toBe("Views are not enabled for the project.");
      expect(await driver.viewsListNames()).toEqual([]);
      expect(await driver.viewsGateManageVisible()).toBe(true);
      expect(await driver.viewsGateManageEnabled()).toBe(true);
    });

    await test.step("the shortcut lands on the project features settings page", async () => {
      await driver.viewsGateManageOpen();
      expect(await driver.viewsGateTitle()).toBe("");
    });

    await test.step("enabling the feature replaces the gate with the list", async () => {
      await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { issue_views_view: true });
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
      expect(await driver.viewsGateTitle()).toBe("");
    });
  }
);

test(
  specTitle(["VIEW-002"], "non-admins see the gate with a disabled shortcut"),
  { tag: specTags(["VIEW-002"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v2b", false);
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-v2b-member");
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-v2b-guest");

    for (const user of [member, guest]) {
      await driver.openAuthenticated(
        `/${harness.workspaceSlug}/projects/${harness.projectId}/views`,
        browserSessionCookies(user)
      );
      await expect.poll(() => driver.viewsGateTitle(), POLL).toBe("Views are not enabled for the project.");
      expect(await driver.viewsGateManageVisible()).toBe(true);
      expect(await driver.viewsGateManageEnabled()).toBe(false);
    }
  }
);
