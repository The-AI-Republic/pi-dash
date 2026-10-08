// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for the top bar and its sidebar toggle (NEWFRONT-126).
// The bar composes the workspace menu, the collapse toggle, the command
// search box, the inbox link, help and the repository star link, and it
// swaps in a compact account control wherever the sidebar is unmounted.
// The toggle flips the sidebar and always leaves peek cleared. Rows:
// SHELL-065, SHELL-066.
import { test, expect } from "../../fixtures";
import { serverSetTourCompleted, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-065", "SHELL-066"];

test(specTitle(ROWS, "top bar composition and sidebar toggle"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  await test.step("sign in through the UI", async () => {
    await driver.signInWithPassword(seed.email, seed.password);
  });

  // The first-run tour overlay covers the dashboard for fresh users, so clear it.
  const session = await signInSession(seed.email, seed.password);
  await serverSetTourCompleted(session, true);

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
    expect(await driver.inboxDotPresent()).toBe(false);
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
    // Generous budget: under shared-stack contention the notifications
    // page can take a while to mount its bar.
    await expect.poll(async () => (await driver.topBarControls()).starLink, { timeout: 60_000 }).toBe(true);
    const controls = await driver.topBarControls();
    expect(controls.accountFallback).toBe(true);
    expect(controls.inbox).toBe(true);
  });
});
