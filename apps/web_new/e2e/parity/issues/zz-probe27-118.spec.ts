// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 6. Saturday Month-zoom audit: is
// the today-month marker in view (re-center OK?) and where is today's
// week? Deleted before PR.
import { expect, test } from "../fixtures";
import { browserCookies, signInFreshUser } from "../helpers/api";

test("probe27: saturday month re-center audit", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const user = await signInFreshUser(seed.email, seed.password);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.ganttOpenTimeline();
  await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
  await page
    .locator("#gantt-container")
    .locator("xpath=..")
    .locator("xpath=.//div[normalize-space(.)='Month' and contains(@class,'cursor-pointer')]")
    .first()
    .click({ timeout: 120_000 });
  await page.waitForTimeout(2_000);
  const out = await page.evaluate(() => {
    const vw = window.innerWidth;
    const container = document.querySelector("#gantt-container") as HTMLElement | null;
    const full = [...(container?.querySelectorAll('[class*="bg-accent-primary"]') ?? [])].map((el) => {
      const r = (el as HTMLElement).getBoundingClientRect();
      return {
        tag: el.tagName,
        cls: ((el as HTMLElement).className ?? "").slice(0, 90),
        text: ((el as HTMLElement).innerText ?? "").slice(0, 24),
        left: Math.round(r.left),
        right: Math.round(r.right),
        inView: r.left < vw && r.right > 0,
      };
    });
    // Month title blocks: first row above the week cells.
    const titles = [...(container?.querySelectorAll("div.flex.h-6 > div, div.flex.h-7 > div") ?? [])]
      .slice(0, 16)
      .map((el) => {
        const r = (el as HTMLElement).getBoundingClientRect();
        return `${((el as HTMLElement).innerText ?? "").slice(0, 12)}@${Math.round(r.left)}-${Math.round(r.right)}`;
      });
    // Week cells: find index whose label week contains Oct 3 by position:
    // dump labels near the viewport center.
    const cells = [...(container?.querySelectorAll("div.flex.h-5 > div") ?? [])] as HTMLElement[];
    const near = cells
      .map((c, i) => ({ i, r: c.getBoundingClientRect(), t: (c.innerText ?? "").replace(/\s+/g, " ").slice(0, 12) }))
      .filter((c) => c.r.left < vw && c.r.right > 0)
      .map((c) => `${c.i}:${c.t}`);
    return JSON.stringify({
      scroll: container?.scrollLeft ?? -1,
      fullCount: full.length,
      full,
      titles,
      visibleWeeks: near,
      nodeDay: new Date().getDay(),
    });
  });
  console.log(`P27: ${out}`);
});
