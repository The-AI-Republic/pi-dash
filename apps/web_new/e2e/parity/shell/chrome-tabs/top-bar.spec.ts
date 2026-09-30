// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the top bar and its sidebar toggle (NEWFRONT-126).
// The bar composes the workspace menu, the collapse toggle, the command
// search box, the inbox link, help and the repository star link, and it
// swaps in a compact account control wherever the sidebar is unmounted.
// The toggle flips the sidebar and always leaves peek cleared. Rows:
// SHELL-065, SHELL-066.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-065", "SHELL-066"];

test(specTitle(ROWS, "top bar composition and sidebar toggle"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  await test.step("sign in through the UI", async () => {
    await driver.signInWithPassword(seed.email, seed.password);
  });

  await test.step("the bar composes every control on a project page", async () => {
    await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
    const controls = await driver.topBarControls();
    expect(controls).toEqual({
      workspaceMenu: true,
      sidebarToggle: true,
      search: true,
      inbox: true,
      help: true,
      starLink: true,
      accountFallback: false,
    });
  });

  await test.step("no unread dot shows with an empty inbox", async () => {
    const dots = await driver.page.locator(".bg-danger-primary").count();
    expect(dots).toBe(0);
  });

  await test.step("the toggle collapses and reopens the sidebar", async () => {
    const openWidth = await driver.sidebarWidth();
    expect(openWidth).toBeGreaterThan(200);
    await driver.toggleSidebar();
    await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBe(0);
    await driver.toggleSidebar();
    await expect.poll(() => driver.sidebarWidth(), { timeout: 15_000 }).toBeGreaterThan(200);
  });

  await test.step("the compact account control appears where the sidebar is unmounted", async () => {
    await driver.openNotifications(seed.workspaceSlug);
    await expect.poll(() => driver.sidebarPresent(), { timeout: 30_000 }).toBe(false);
    await expect
      .poll(() => driver.page.getByRole("link", { name: "Star us on GitHub" }).count(), { timeout: 30_000 })
      .toBeGreaterThan(0);
    const controls = await driver.topBarControls();
    expect(controls.accountFallback).toBe(true);
    expect(controls.inbox).toBe(true);
  });
});
