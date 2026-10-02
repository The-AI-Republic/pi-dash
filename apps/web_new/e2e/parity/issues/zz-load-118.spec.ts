// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY LOAD PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import { browserCookies, signInFreshUser } from "../helpers/api";

test("probe: issues page load", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const user = await signInFreshUser(seed.email, seed.password);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  for (let i = 0; i < 12; i++) {
    await page.waitForTimeout(15_000);
    const switcher = await page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button").count();
    const kanban = await page.locator('div[id*="__"]').count();
    console.log(`PROBE-T${(i + 1) * 15}s url=${page.url().slice(-60)} switcher=${switcher} kanbanids=${kanban}`);
    if (switcher > 0) break;
  }
  console.log(
    "PROBE-BODY:" +
      JSON.stringify(
        (
          await page
            .locator("body")
            .innerText()
            .catch(() => "")
        ).slice(0, 400)
      )
  );
  await page.screenshot({ path: "/tmp/recon-118/issues-load.png" });
  console.log("PROBE-DONE");
});
