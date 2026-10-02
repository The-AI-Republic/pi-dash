// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY CYCLE-PAGE PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import { browserCookies, serverCreateCycle, serverDeleteCycle, signInFreshUser } from "../helpers/api";

test("probe: cycle page board", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const user = await signInFreshUser(seed.email, seed.password);
  const cycle = await serverCreateCycle(
    seed.workspaceSlug,
    seed.projectId,
    user.cookie,
    "Probe cycle 118",
    "2026-01-05",
    "2026-01-12"
  );
  console.log("PROBE-CYCLE:" + JSON.stringify(cycle));
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`,
    browserCookies(user)
  );
  await page.waitForTimeout(30_000);
  console.log("PROBE-URL:" + page.url());
  console.log("PROBE-BOARD:" + String(await driver.kanbanBoardVisible()));
  console.log("PROBE-COLS:" + JSON.stringify(await driver.kanbanColumns().catch((e) => String(e).slice(0, 200))));
  console.log(
    "PROBE-MENU:" + JSON.stringify(await driver.kanbanHeaderMenuItems("Todo").catch((e) => String(e).slice(0, 200)))
  );
  await page.screenshot({ path: "/tmp/recon-118/cycle-page.png" });
  await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, user.cookie);
  console.log("PROBE-DONE");
});
