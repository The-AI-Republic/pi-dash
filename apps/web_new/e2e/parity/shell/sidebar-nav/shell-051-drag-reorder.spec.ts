// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): project rows drag-reorder with
// persistence.
// Observed on the running old app: joined-project rows drag above each
// other from a hover-revealed handle; the new order survives a reload
// because the row order persists on the server through the project
// user-properties endpoint; a failed save toasts an error and keeps the
// old order. Row: SHELL-051.
import { test, expect } from "../../fixtures";
import { deleteProject, ensureProject, ownerSession, patchUserProperties } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-051"];
const SECOND_NAME = "Parity Sidebar Two";
const SECOND_CODE = "PAR_SB";

async function sidebarProjectOrder(
  driver: { sidebarLinkTexts(): Promise<string[]> },
  names: string[]
): Promise<string[]> {
  const links = await driver.sidebarLinkTexts();
  return links.filter((link) => names.includes(link));
}

test(
  specTitle(ROWS, "project rows drag-reorder with persistence"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    const second = await test.step("provision a second project", async () =>
      ensureProject(seed.workspaceSlug, session, SECOND_NAME, SECOND_CODE));
    // Uncapped baseline: an interrupted overflow run may have left a cap.
    await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("drag the seeded project above the second", async () => {
      // Re-guard the cap (shared seed user): hidden rows cannot be dragged.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(SECOND_NAME);
      // Freshly joined projects sort first.
      expect(await sidebarProjectOrder(driver, [seed.projectName, SECOND_NAME])).toEqual([
        SECOND_NAME,
        seed.projectName,
      ]);
      // A drag can silently not take under load, so drive it again while the
      // order still reads pre-drag instead of failing on the first attempt.
      const wanted = [seed.projectName, SECOND_NAME];
      let order: string[] = [];
      for (let attempt = 0; attempt < 2; attempt += 1) {
        await driver.dragSidebarProjectBefore(seed.projectName, SECOND_NAME);
        const deadline = Date.now() + 15_000;
        do {
          order = await sidebarProjectOrder(driver, [seed.projectName, SECOND_NAME]);
          if (order.join("\n") === wanted.join("\n")) break;
          await driver.page.waitForTimeout(1000);
        } while (Date.now() < deadline);
        if (order.join("\n") === wanted.join("\n")) break;
      }
      expect(order).toEqual(wanted);
    });

    await test.step("the new order survives a reload", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(SECOND_NAME);
      expect(await sidebarProjectOrder(driver, [seed.projectName, SECOND_NAME])).toEqual([
        seed.projectName,
        SECOND_NAME,
      ]);
    });

    await test.step("a failed save toasts and keeps the old order", async () => {
      // Hold the order-save endpoint closed so the drop's write fails: the
      // app must toast an error and revert to the pre-drag order.
      const pattern = "**/api/workspaces/*/projects/*/user-properties/";
      await driver.page.route(pattern, async (route) => {
        if (route.request().method() === "PATCH") {
          await route.abort();
        } else {
          await route.continue();
        }
      });
      try {
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(SECOND_NAME);
        const before = [seed.projectName, SECOND_NAME];
        expect(await sidebarProjectOrder(driver, before)).toEqual(before);
        await driver.dragSidebarProjectBefore(SECOND_NAME, seed.projectName);
        await expect.poll(() => driver.isToastVisible("Something went wrong"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => sidebarProjectOrder(driver, before), { timeout: 30_000 }).toEqual(before);
      } finally {
        await driver.page.unroute(pattern);
      }
    });

    await test.step("restore the seeded baseline", async () => {
      await deleteProject(seed.workspaceSlug, second.id, session);
    });
  }
);
