// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for the collapsible sidebar shell (NEWFRONT-126). The
// shell carries a titled header with quick actions, a scrollable middle
// and a single bottom account area; its right edge drag-resizes between a
// minimum and a maximum width and double-click collapses it; narrow screens
// float it above content and collapse it on outside taps; collapse survives
// reloads. Rows: SHELL-067, SHELL-068, SHELL-069, SHELL-098.
import { test, expect } from "../../fixtures";
import { serverSetTourCompleted, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-067", "SHELL-068", "SHELL-069", "SHELL-098"];

test(
  specTitle(ROWS, "collapsible sidebar shell, resize, float and persistence"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    const session = await signInSession(seed.email, seed.password);
    await serverSetTourCompleted(session, true);

    await test.step("the shell carries title, quick actions and one account area", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarBrandVisible(), { timeout: 15_000 }).toBe(true);
      await expect
        .poll(() => driver.sidebarQuickActionNames(), { timeout: 15_000 })
        .toEqual(expect.arrayContaining(["New work item"]));
      expect(await driver.sidebarAccountButtonCount()).toBe(1);
      expect((await driver.topBarControls()).accountFallback).toBe(false);
    });

    await test.step("drag resizes within its clamps", async () => {
      const startWidth = await driver.sidebarWidth();
      expect(startWidth).toBeGreaterThan(200);
      await driver.dragSidebarGripBy(400);
      const grown = await driver.sidebarWidth();
      expect(grown).toBeGreaterThan(startWidth ?? 0);
      expect(grown).toBeLessThanOrEqual(350);
      await driver.dragSidebarGripBy(-400);
      const shrunk = await driver.sidebarWidth();
      expect(shrunk).toBeLessThan(grown ?? 400);
      expect(shrunk).toBeGreaterThanOrEqual(236);
    });

    await test.step("double-click on the edge collapses and reopens", async () => {
      await driver.doubleClickSidebarGrip();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      // The collapsed edge offers no visible grip to double-click back, so the
      // toggle reopens the shell.
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
    });

    await test.step("no hover peek overlay appears after collapse", async () => {
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      await driver.hoverCollapsedEdge();
      await driver.page.waitForTimeout(2500);
      expect(await driver.sidebarWidth()).toBe(0);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
    });

    await test.step("narrow screens auto-collapse and collapse on outside taps", async () => {
      await driver.setViewportSize(500, 800);
      await expect.poll(() => driver.sidebarWidth(), { timeout: 20_000 }).toBe(0);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(100);
      await driver.clickOutsideSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      await driver.setViewportSize(1280, 720);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
    });

    await test.step("collapse survives a reload", async () => {
      // A bare reload can land on the signed-out entry before the session
      // read resolves, so revisit the address: a fresh load still restores
      // the persisted collapse flag from local storage either way.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarWidth(), { timeout: 20_000 }).toBeGreaterThan(200);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarWidth(), { timeout: 30_000 }).toBe(0);
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarWidth(), { timeout: 30_000 }).toBeGreaterThan(200);
    });
  }
);
