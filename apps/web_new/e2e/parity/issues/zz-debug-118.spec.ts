// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY DEBUG PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateProject,
  serverDefaultStateId,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe: scratch swimlane DOM", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `DB probe ${suffix}`, `DB${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  console.log("DBG-STATES:" + JSON.stringify(states.map((s) => s.name)));
  const home = await serverDefaultStateId(seed.workspaceSlug, projectId, owner.cookie);
  const label = await serverCreateLabel(seed.workspaceSlug, projectId, owner.cookie, `DB label ${suffix}`);
  const aId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `DB a ${suffix}`, home);
  await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `DB b ${suffix}`, home);
  await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `DB c ${suffix}`, home);
  await serverPatchIssue(seed.workspaceSlug, projectId, aId, { label_ids: [label.id] }, owner.cookie);
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      group_by: "state",
      sub_group_by: "labels",
      order_by: "sort_order",
      show_empty_groups: true,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  for (let i = 0; i < 16; i++) {
    await page.waitForTimeout(15_000);
    const n = await page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button").count();
    console.log(`DBG-T${(i + 1) * 15}s switcher=${n} url=${page.url().slice(-40)}`);
    if (n > 0) break;
  }
  await page.screenshot({ path: "/tmp/recon-118/db-swimlane.png" });
  const dump = await page.evaluate(() => {
    const main = document.querySelector("main");
    const topSticky = [...document.querySelectorAll('div.sticky.top-0[class*="z-[4]"]')];
    const laneBars = [...document.querySelectorAll('div[class*="top-[50px]"]')];
    const inners = [...(main?.querySelectorAll('div[id*="__"]') ?? [])];
    return {
      bodyHead: (document.body.innerText ?? "").slice(0, 300),
      topSticky: topSticky.map((el) => ({ kids: el.children.length, text: (el.textContent ?? "").slice(0, 120) })),
      laneBars: laneBars.map((el) => ({ text: (el.textContent ?? "").slice(0, 100) })),
      innerIds: inners.map((el) => el.getAttribute("id")).slice(0, 12),
      flatOuters: main?.querySelectorAll("div.group.relative.flex.flex-shrink-0.flex-col").length ?? -1,
    };
  });
  console.log("DBG-DUMP:" + JSON.stringify(dump));
  // Lane toggle attempt with before/after chevron readout.
  try {
    const laneName = label.name;
    console.log("DBG-COLLAPSED-BEFORE:" + String(await driver.kanbanSwimlaneCollapsed(laneName)));
    await driver.kanbanToggleSwimlane(laneName);
    console.log("DBG-COLLAPSED-AFTER:" + String(await driver.kanbanSwimlaneCollapsed(laneName)));
  } catch (error) {
    console.log("DBG-TOGGLE-ERR:" + String(error).slice(0, 300));
  }
  console.log("DBG-DONE");
});
