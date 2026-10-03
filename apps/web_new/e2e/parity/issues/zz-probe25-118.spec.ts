// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 6. Post-zoom-switch audit: when do
// the re-center scroll and the today highlight land after a zoom click?
// Deleted before PR.
import type { Page } from "@playwright/test";
import { expect, test } from "../fixtures";
import { browserCookies, signInFreshUser } from "../helpers/api";

async function sample(page: Page): Promise<string> {
  return await page.evaluate(() => {
    const container = document.querySelector("#gantt-container") as HTMLElement | null;
    const firstCell = container?.querySelector("div.flex.h-5 > div");
    const first = (firstCell?.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 12);
    const marked = [...(container?.querySelectorAll("div.bg-accent-primary\\/20") ?? [])] as HTMLElement[];
    const vw = window.innerWidth;
    const vis = marked.filter((el) => {
      const r = el.getBoundingClientRect();
      return r.left < vw && r.right > 0;
    }).length;
    return JSON.stringify({
      scroll: container?.scrollLeft ?? -1,
      first,
      marked: marked.length,
      vis,
    });
  });
}

async function clickZoom(page: Page, view: string): Promise<void> {
  await page
    .locator("#gantt-container")
    .locator("xpath=..")
    .locator(`xpath=.//div[normalize-space(.)='${view}' and contains(@class,'cursor-pointer')]`)
    .first()
    .click({ timeout: 120_000 });
}

test("probe25: post-switch today timeline", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const user = await signInFreshUser(seed.email, seed.password);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.ganttOpenTimeline();
  await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
  console.log(`P25-BASE: ${await sample(page)}`);
  for (const view of ["Month", "Quarter", "Week"]) {
    await clickZoom(page, view);
    for (let i = 0; i < 30; i += 1) {
      await page.waitForTimeout(200);
      const s = await sample(page);
      if (i < 8 || i % 5 === 0) console.log(`P25-${view}-T${i}: ${s}`);
    }
  }
});
