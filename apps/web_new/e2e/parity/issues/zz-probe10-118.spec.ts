// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. ISS-031 zero-cards replica with
// per-lane dumps. Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateProject,
  serverDeleteLabel,
  serverDeleteProject,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe10: ISS-031 replica with dumps", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6);
  const label = await serverCreateLabel(seed.workspaceSlug, seed.projectId, owner.cookie, `KB fold ${suffix}`);
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
  const first = rows.find((r) => r.name === (seed.issueNames[0] ?? ""));
  if (!first) throw new Error("[probe] no seed issue");
  await serverPatchIssue(seed.workspaceSlug, seed.projectId, first.id, { label_ids: [label.id] }, owner.cookie);
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      group_by: "state",
      sub_group_by: "labels",
      show_empty_groups: true,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.kanbanOpenBoard();
  const lanes = await driver.kanbanSwimlanes();
  console.log(`P10-LANES:${JSON.stringify(lanes)}`);
  const cards0 = await driver.kanbanCards();
  console.log(`P10-CARDS-BEFORE:${JSON.stringify(cards0.map((c) => c.name))}`);
  const bodies0 = await page.locator('main div[id*="__"]').count();
  console.log(`P10-BODIES-BEFORE:${bodies0}`);
  console.log(`P10-COLLAPSED-BEFORE:${await driver.kanbanSwimlaneCollapsed(label.name)}`);
  const bars = await page.locator('main div[class*="top-[50px]"]').count();
  console.log(`P10-BARS:${bars}`);
  await driver.kanbanToggleSwimlane(label.name);
  console.log(`P10-COLLAPSED-AFTER:${await driver.kanbanSwimlaneCollapsed(label.name)}`);
  const lanesAfter = await driver.kanbanSwimlanes();
  console.log(`P10-LANES-AFTER:${JSON.stringify(lanesAfter)}`);
  const cards1 = await driver.kanbanCards();
  console.log(`P10-CARDS-AFTER:${JSON.stringify(cards1.map((c) => c.name))}`);
  const bodies1 = await page.locator('main div[id*="__"]').count();
  console.log(`P10-BODIES-AFTER:${bodies1}`);
  // Per-lane body presence via the wrapper structure the driver uses.
  const perLane = await page.evaluate(() => {
    const out: Array<{ text: string; bodies: number }> = [];
    const all = [...document.querySelectorAll('main div[class*="top-[50px]"]')];
    for (const bar of all) {
      const wrap = bar.parentElement;
      const bodies = wrap ? wrap.querySelectorAll('div[id*="__"]').length : -1;
      out.push({ text: (bar.textContent ?? "").slice(0, 60).replace(/\s+/g, " "), bodies });
    }
    return out;
  });
  console.log(`P10-PERLANE:${JSON.stringify(perLane)}`);
  await serverPatchIssue(seed.workspaceSlug, seed.projectId, first.id, { label_ids: [] }, owner.cookie);
  await serverDeleteLabel(seed.workspaceSlug, seed.projectId, label.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: before.displayFilters,
    display_properties: before.displayProperties,
  });
  console.log("P10-DONE");
});

test("probe10b: ISS-043 card presence sampling + auto-scroll", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  const names: string[] = [];
  for (let i = 0; i < 40; i += 1) {
    const name = `KB probe ${suffix} ${String(i + 1).padStart(2, "0")}`;
    names.push(name);
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, name, home.id);
  }
  const last = names[names.length - 1] ?? "";
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  for (let attempt = 0; attempt < 6; attempt += 1) {
    await driver.kanbanColumnScrollEnd(home.name);
    const cards = await driver.kanbanColumnCards(home.name);
    console.log(`P10B-SCROLL${attempt}:n=${cards.length} hasLast=${cards.includes(last)}`);
    if (cards.includes(last)) break;
  }
  for (let i = 0; i < 6; i += 1) {
    await page.waitForTimeout(10_000);
    const n = await page.locator('a[id^="issue_"]').count();
    const cards = await driver.kanbanColumnCards(home.name).catch(() => ["ERR"]);
    console.log(`P10B-T${i}:mounted=${n} colN=${cards.length} hasLast=${cards.includes(last)}`);
  }
  const beforeX = await driver.kanbanBoardScroll();
  const colBefore = await driver.kanbanColumnScroll(home.name);
  console.log(`P10B-SCROLL-READ:board=${JSON.stringify(beforeX)} col=${JSON.stringify(colBefore)}`);
  const held = (await driver.kanbanColumnCards(home.name)).slice(-1)[0] ?? "";
  console.log(`P10B-HELD:${held}`);
  await driver.kanbanDragHoldNearEdge(held, "right", 2500);
  const afterX = await driver.kanbanBoardScroll();
  console.log(`P10B-AFTER-H:board=${JSON.stringify(afterX)}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P10B-DONE");
});
