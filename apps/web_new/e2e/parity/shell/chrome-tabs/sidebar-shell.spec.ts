// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the collapsible sidebar shell (NEWFRONT-126). The
// shell carries a titled header with quick actions, a scrollable middle
// and a single bottom account area; its right edge drag-resizes between a
// minimum and a maximum width and double-click collapses it; narrow screens
// float it above content and collapse it on outside taps; collapse survives
// reloads. Rows: SHELL-067, SHELL-068, SHELL-069, SHELL-098.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-067", "SHELL-068", "SHELL-069", "SHELL-098"];

test(
  specTitle(ROWS, "collapsible sidebar shell, resize, float and persistence"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the shell carries title, quick actions and one account area", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      const sidebar = driver.page.locator("#main-sidebar");
      await expect(sidebar.getByText("Pi Dash", { exact: true }).first()).toBeVisible();
      await expect(sidebar.getByRole("button", { name: "New work item" })).toBeVisible();
      expect(await sidebar.getByRole("button").filter({ hasText: "@" }).count()).toBe(1);
      expect((await driver.topBarControls()).accountFallback).toBe(false);
    });

    await test.step("drag resizes within its clamps", async () => {
      const grip = driver.page.getByRole("separator", { name: "Resize sidebar" }).first();
      const dragGripBy = async (dx: number) => {
        const box = await grip.boundingBox();
        expect(box).not.toBeNull();
        const x = (box?.x ?? 0) + (box?.width ?? 0) / 2;
        const y = (box?.y ?? 0) + (box?.height ?? 0) / 2;
        await driver.page.mouse.move(x, y);
        await driver.page.mouse.down();
        await driver.page.mouse.move(x + dx, y, { steps: 10 });
        await driver.page.mouse.up();
      };
      const startWidth = await driver.sidebarWidth();
      expect(startWidth).toBeGreaterThan(200);
      await dragGripBy(400);
      const grown = await driver.sidebarWidth();
      expect(grown).toBeGreaterThan(startWidth ?? 0);
      expect(grown).toBeLessThanOrEqual(350);
      await dragGripBy(-400);
      const shrunk = await driver.sidebarWidth();
      expect(shrunk).toBeLessThan(grown ?? 400);
      expect(shrunk).toBeGreaterThanOrEqual(236);
    });

    await test.step("double-click on the edge collapses and reopens", async () => {
      const doubleClickGrip = async () => {
        const grip = driver.page.getByRole("separator", { name: "Resize sidebar" }).first();
        const box = await grip.boundingBox();
        expect(box).not.toBeNull();
        await driver.page.mouse.dblclick((box?.x ?? 0) + (box?.width ?? 0) / 2, (box?.y ?? 0) + 100);
      };
      await doubleClickGrip();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      // The collapsed edge offers no visible grip to double-click back, so the
      // toggle reopens the shell.
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
    });

    await test.step("no hover peek overlay appears after collapse", async () => {
      await driver.toggleSidebar();
      await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
      await driver.page.mouse.move(4, 400);
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
      // Raw mouse events: the floating shell animates under the cursor, which
      // defeats actionability checks, while the outside detector only needs
      // the press itself.
      await driver.page.mouse.click(450, 400);
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
