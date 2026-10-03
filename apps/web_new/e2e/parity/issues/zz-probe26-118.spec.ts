// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 6. Month-zoom today-marking audit:
// which classes mark today at Month, and what range renders. Deleted
// before PR.
import type { Page } from "@playwright/test";
import { expect, test } from "../fixtures";
import { browserCookies, signInFreshUser } from "../helpers/api";

async function dump(page: Page, label: string): Promise<void> {
  const out = await page.evaluate(() => {
    const container = document.querySelector("#gantt-container") as HTMLElement | null;
    const accented = new Set<string>();
    container?.querySelectorAll('[class*="accent"]').forEach((el) => {
      (el as HTMLElement).classList.forEach((c) => {
        if (c.includes("accent")) accented.add(c);
      });
    });
    const cells = [...(container?.querySelectorAll("div.flex.h-5 > div") ?? [])];
    const texts = cells.map((c) => (c.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 14));
    // Find the cell whose week contains Oct 3 (today): weeks look like "29-4w13".
    const today = new Date();
    const todayIdx = texts.findIndex((t) => {
      const m = t.match(/^(\d+)-(\d+)w\d+/);
      if (!m) return false;
      // Week spans month boundary or not; crude: check via title attr instead.
      return false;
    });
    const titles = cells.slice(0, 6).map((c) => (c.getAttribute("title") ?? "").slice(0, 40));
    const cellClasses = cells.slice(0, 6).map((c) => ((c as HTMLElement).className ?? "").slice(0, 120));
    return JSON.stringify({
      scroll: container?.scrollLeft ?? -1,
      scrollW: container?.scrollWidth ?? -1,
      clientW: container?.clientWidth ?? -1,
      accented: [...accented].sort(),
      nCells: cells.length,
      first3: texts.slice(0, 3),
      last3: texts.slice(-3),
      todayIdx,
      todayStr: `${today.getFullYear()}-${today.getMonth() + 1}-${today.getDate()}`,
      titles,
      cellClasses,
    });
  });
  console.log(`P26-${label}: ${out}`);
}

test("probe26: month today-marking audit", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const user = await signInFreshUser(seed.email, seed.password);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.ganttOpenTimeline();
  await expect.poll(() => driver.ganttSidebarRows(), { timeout: 120_000 }).toHaveLength(3);
  await dump(page, "WEEK");
  await page
    .locator("#gantt-container")
    .locator("xpath=..")
    .locator("xpath=.//div[normalize-space(.)='Month' and contains(@class,'cursor-pointer')]")
    .first()
    .click({ timeout: 120_000 });
  await page.waitForTimeout(2_000);
  await dump(page, "MONTH");
  // Scroll across the range: does any marking appear?
  await page.evaluate(() => {
    document.querySelector("#gantt-container")?.scrollTo({ left: 0 });
  });
  await page.waitForTimeout(1_000);
  await dump(page, "MONTH-LEFT0");
});
