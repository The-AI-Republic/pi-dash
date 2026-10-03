// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3d. Guest board rendering facts.
// Resume aid; deleted before PR.
import { test } from "../fixtures";
import { browserCookies, serverProjectUserProperties, signInFreshUser } from "../helpers/api";

test("probe35b: guest opens the issues board", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  if (!seed.guestEmail || !seed.guestPassword) throw new Error("[probe] no guest in seed");
  const guest = await signInFreshUser(seed.guestEmail, seed.guestPassword);
  console.log(`P35B-GUEST-COOKIE:${guest.cookie.slice(0, 40)}...`);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(guest));
  await page.waitForTimeout(10_000);
  console.log(`P35B-URL:${page.url()}`);
  const switcher = page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button");
  console.log(`P35B-SWITCHER-COUNT:${await switcher.count()}`);
  for (let i = 0; i < (await switcher.count()); i += 1) {
    console.log(`P35B-BTN-${i}:${(await switcher.nth(i).getAttribute("class"))?.slice(0, 120)}`);
  }
  console.log(`P35B-ACTIVE:${await driver.boardActiveLayout()}`);
  console.log(`P35B-KANBAN-VISIBLE:${await driver.kanbanBoardVisible()}`);
  console.log(`P35B-BODIES:${await page.getByRole("main").locator('div[id*="__"]').count()}`);
  console.log(
    `P35B-MAIN-TEXT:${JSON.stringify(
      await page
        .getByRole("main")
        .innerText()
        .catch(() => "")
    ).slice(0, 600)}`
  );
  // Can the guest's prefs be read/patched at all?
  try {
    const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, guest.cookie);
    console.log(`P35B-PREFS-OK:${JSON.stringify(props.displayFilters).slice(0, 300)}`);
  } catch (error) {
    console.log(`P35B-PREFS-ERR:${error instanceof Error ? error.message : error}`);
  }
});
