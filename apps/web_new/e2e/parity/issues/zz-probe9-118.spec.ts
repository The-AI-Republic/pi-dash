// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. ISS-030/031 collapse persistence +
// reload settle, and ISS-043 virtualization windowing. Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverCreateState,
  serverDeleteProject,
  serverListStates,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe9a: collapse persist + reload settle", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      group_by: "state",
      sub_group_by: null,
      show_empty_groups: true,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.kanbanOpenBoard();
  const cols = await driver.kanbanColumns();
  console.log(`P9A-COLS:${JSON.stringify(cols.map((c) => c.name))}`);
  const name = cols[0]?.name ?? "";
  await driver.kanbanToggleColumn(name);
  console.log(`P9A-COLLAPSED:${await driver.kanbanColumnCollapsed(name)}`);
  const bodies = await page.locator('main div[id*="__"]').count();
  console.log(`P9A-BODIES-COLLAPSED:${bodies}`);
  const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  console.log(`P9A-PROPS:${JSON.stringify(props.displayFilters)}`);
  const ls = await page.evaluate(() => Object.keys(window.localStorage));
  console.log(`P9A-LS-KEYS:${JSON.stringify(ls)}`);
  for (const key of ls) {
    if (/kanban|collapse|board|filter/i.test(key)) {
      const val = await page.evaluate((k) => window.localStorage.getItem(k), key);
      console.log(`P9A-LS:${key}=${(val ?? "").slice(0, 400)}`);
    }
  }
  await driver.boardReloadIssues();
  for (let i = 0; i < 12; i += 1) {
    await page.waitForTimeout(5_000);
    const active = await driver.boardActiveLayout().catch((e: unknown) => `ERR:${String(e).slice(0, 80)}`);
    const vis = await driver.kanbanBoardVisible().catch(() => false);
    const n = await page
      .locator('main div[id*="__"]')
      .count()
      .catch(() => -1);
    const mains = await page
      .locator("main")
      .count()
      .catch(() => -1);
    console.log(`P9A-T${i}:active=${active} vis=${vis} bodies=${n} mains=${mains} url=${page.url().slice(-60)}`);
    if (active === "kanban" && vis) break;
  }
  const strip = await page
    .locator("main")
    .innerHTML()
    .catch((e: unknown) => `ERR:${String(e).slice(0, 120)}`);
  console.log(`P9A-MAIN:${strip.slice(0, 3000)}`);
  await driver
    .kanbanToggleColumn(name)
    .catch((e: unknown) => console.log(`P9A-UNCOLLAPSE-ERR:${String(e).slice(0, 120)}`));
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: before.displayFilters,
    display_properties: before.displayProperties,
  });
  console.log("P9A-DONE");
});

test("probe9b: virtualization windowing", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  for (let i = 0; i < 60; i += 1) {
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe ${suffix} ${i + 1}`, home.id);
  }
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  const mounted0 = await page.locator('a[id^="issue_"]').count();
  console.log(`P9B-MOUNTED-AT-REST:${mounted0}`);
  const geom0 = await page.evaluate(() => {
    const col = document.querySelector('main div[id*="__"]');
    const scroller = col?.closest(
      "div.overflow-y-auto, div.overflow-auto, div[data-overlayscrollbars-viewport]"
    ) as HTMLElement | null;
    const host = scroller ?? (col?.parentElement as HTMLElement | null);
    return {
      col: col ? `${col.scrollHeight}x${col.clientHeight}` : "none",
      host: host ? `${host.scrollHeight}x${host.clientHeight}@${host.scrollTop}` : "none",
      hostCls: host ? host.className.slice(0, 120) : "none",
    };
  });
  console.log(`P9B-GEOM0:${JSON.stringify(geom0)}`);
  await driver.kanbanColumnScrollEnd(home.name);
  await page.waitForTimeout(3_000);
  const mounted1 = await page.locator('a[id^="issue_"]').count();
  const geom1 = await page.evaluate(() => {
    const col = document.querySelector('main div[id*="__"]');
    const all = [...document.querySelectorAll("main div")].filter((d) => {
      const el = d as HTMLElement;
      return el.scrollHeight > el.clientHeight + 50;
    });
    return { col: col ? `${col.scrollHeight}x${col.clientHeight}` : "none", scrollers: all.length };
  });
  console.log(`P9B-MOUNTED-AFTER-SCROLL:${mounted1} GEOM1:${JSON.stringify(geom1)}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P9B-DONE");
});

test("probe9c: create scratch state for ISS-038 style check", async ({ seed }) => {
  test.setTimeout(120_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const st = await serverCreateState(seed.workspaceSlug, seed.projectId, owner.cookie, "ZZ temp", "unstarted");
  console.log(`P9C-CREATED:${st.name}`);
  const res = await fetch(
    `http://localhost:18032/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/states/${st.id}/`,
    { method: "DELETE", headers: { cookie: owner.cookie } }
  );
  console.log(`P9C-DEL:${res.status}`);
});
