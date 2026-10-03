// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3e. Cycle header create click facts.
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

test("probe36b: cycle header create click", async ({ driver, seed, page }) => {
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
  const columns = await driver.kanbanColumns();
  console.log(`P36B-COLUMNS:${JSON.stringify(columns)}`);
  // Find the cycle column outer by header text.
  const outers = page.getByRole("main").locator("div.group.relative.flex.flex-shrink-0.flex-col");
  const n = await outers.count();
  for (let i = 0; i < n; i += 1) {
    const header = outers.nth(i).locator(":scope > div.sticky").first();
    const htext = ((await header.innerText().catch(() => "")) ?? "").replace(/\s+/g, " ").slice(0, 80);
    const btns = outers.nth(i).locator(":scope > div.sticky button");
    const spans = header.locator("span.cursor-pointer");
    console.log(
      `P36B-OUTER-${i}: header=${JSON.stringify(htext)} buttons=${await btns.count()} spans=${await spans.count()}`
    );
    if (htext.includes(cycle.name)) {
      for (let b = 0; b < (await btns.count()); b += 1) {
        console.log(
          `P36B-BTN-${b}: ${(
            (await btns
              .nth(b)
              .innerText()
              .catch(() => "")) ?? ""
          ).slice(0, 60)} | ${
            (await btns
              .nth(b)
              .getAttribute("aria-label")
              .catch(() => "")) ?? ""
          }`
        );
      }
      // Click the same target the driver would.
      if ((await btns.count()) > 1) {
        await btns.nth(1).click({ timeout: 30_000 });
        console.log("P36B-CLICKED:button[1]");
      } else {
        await spans.first().click({ timeout: 30_000 });
        console.log("P36B-CLICKED:span");
      }
      await page.waitForTimeout(3_000);
      console.log(`P36B-MENUS:${await page.locator('[role="menu"]').count()}`);
      console.log(`P36B-MENUITEMS:${await page.locator('[role="menuitem"]').count()}`);
      console.log(`P36B-DIALOGS:${await page.getByRole("dialog").count()}`);
      console.log(`P36B-ASSIGNEES:${await page.getByPlaceholder("Assignees").count()}`);
      const menuText = await page
        .locator('[role="menu"]')
        .first()
        .innerText()
        .catch(() => "");
      console.log(`P36B-MENU-TEXT:${JSON.stringify(menuText).slice(0, 400)}`);
    }
  }
  await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});
