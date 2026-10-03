// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3g. Cycle detail board rendering.
// Resume aid; deleted before PR.
import { test, expect } from "../fixtures";
import {
  browserCookies,
  serverAddIssuesToCycle,
  serverCreateCycle,
  serverDeleteCycle,
  serverIssues,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe36d: cycle detail board", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6);
  const cycle = await serverCreateCycle(
    seed.workspaceSlug,
    seed.projectId,
    owner.cookie,
    `KB cycle ${suffix}`,
    "2026-10-20",
    "2026-11-20"
  );
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
  const first = rows.find((r) => r.name === (seed.issueNames[0] ?? ""));
  if (!first) throw new Error("[probe] no seed issue");
  await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycle.id, [first.id], owner.cookie);
  const user = await signInFreshUser(seed.email, seed.password);
  // Cycle-scoped layout prefs live on the cycle user-properties endpoint;
  // set the project default too so a shared root still renders kanban.
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", group_by: "state", show_empty_groups: true },
  });
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`,
    browserCookies(user)
  );
  await page.waitForTimeout(15_000);
  console.log(`P36D-URL:${page.url()}`);
  console.log(
    `P36D-SWITCHER:${await page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button").count()}`
  );
  console.log(
    `P36D-MAIN-TEXT:${JSON.stringify(
      await page
        .getByRole("main")
        .innerText()
        .catch(() => "")
    ).slice(0, 500)}`
  );
  try {
    await driver.kanbanOpenBoard();
    await expect.poll(() => driver.kanbanColumns(), { timeout: 60_000 }).not.toHaveLength(0);
    console.log(`P36D-COLUMNS:${JSON.stringify(await driver.kanbanColumns())}`);
    const column = (await driver.kanbanColumns())[0]?.name ?? "";
    console.log(`P36D-HEADER-CREATE:${await driver.kanbanHeaderCreateVisible(column)}`);
    await driver.kanbanHeaderCreate(column);
    console.log(`P36D-MENU-ITEMS:${JSON.stringify(await driver.kanbanHeaderMenuItems(column))}`);
  } catch (error) {
    console.log(`P36D-ERR:${error instanceof Error ? error.message.split("\n")[0] : error}`);
  }
  await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});
