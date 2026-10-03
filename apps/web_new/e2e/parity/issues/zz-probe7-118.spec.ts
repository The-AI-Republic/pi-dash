// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3f. Cycle "+" dialog identity.
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

test("probe36c: what the cycle plus opens", async ({ driver, seed, page }) => {
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
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", group_by: "cycle", show_empty_groups: true },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.kanbanOpenBoard();
  await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
  const outers = page.getByRole("main").locator("div.group.relative.flex.flex-shrink-0.flex-col");
  const n = await outers.count();
  for (let i = 0; i < n; i += 1) {
    const header = outers.nth(i).locator(":scope > div.sticky").first();
    const htext = ((await header.innerText().catch(() => "")) ?? "").replace(/\s+/g, " ");
    if (!htext.includes(cycle.name)) continue;
    const btns = outers.nth(i).locator(":scope > div.sticky button");
    await btns.nth(1).click({ timeout: 30_000 });
    await page.waitForTimeout(3_000);
    const dialogs = page.getByRole("dialog");
    console.log(`P36C-DIALOGS:${await dialogs.count()}`);
    for (let d = 0; d < (await dialogs.count()); d += 1) {
      const t = await dialogs
        .nth(d)
        .innerText()
        .catch(() => "");
      console.log(`P36C-DLG-${d}:${JSON.stringify(t).slice(0, 600)}`);
    }
    console.log(
      `P36C-NOTIF:${JSON.stringify(
        await page
          .locator('[aria-label="Notifications"]')
          .innerText()
          .catch(() => "")
      ).slice(0, 300)}`
    );
    console.log(`P36C-MENUS:${await page.locator('[role="menu"]').count()}`);
    await page.screenshot({ path: "test-results/probe36c.png" });
  }
  await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});
