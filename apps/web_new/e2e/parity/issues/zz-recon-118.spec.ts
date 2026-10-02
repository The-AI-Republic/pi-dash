// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY RECON PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import { browserCookies, signInFreshUser, signInSession } from "../helpers/api";

test("recon: gantt reload visibility", async ({ driver, seed, page }) => {
  test.setTimeout(600_000);
  const session = await signInSession(seed.email, seed.password);
  const user = await signInFreshUser(seed.email, seed.password);
  const apiBase = process.env["PARITY_API_URL"] ?? "http://localhost:18032";
  const base = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}`;
  page.on("console", (m) => {
    if (m.type() === "error") console.log("CONSOLE-ERR:", m.text().slice(0, 250));
  });
  page.on("response", (r) => {
    if (r.status() >= 400 && r.url().includes("/api/"))
      console.log(`HTTP${r.status()} ${r.url().slice(r.url().indexOf("/api/"), r.url().indexOf("/api/") + 120)}`);
  });
  const r = await fetch(`${base}/user-properties/`, {
    method: "PATCH",
    headers: { cookie: session, "content-type": "application/json" },
    body: JSON.stringify({
      display_filters: {
        layout: "gantt_chart",
        group_by: "state",
        sub_group_by: null,
        order_by: "sort_order",
        show_empty_groups: true,
      },
    }),
  });
  console.log("PATCH-STATUS:", r.status);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  for (let i = 0; i < 24; i++) {
    await page.waitForTimeout(5000);
    if ((await page.locator("#gantt-container").count()) > 0) {
      console.log(`GANTT-RENDERED after ${(i + 1) * 5}s`);
      break;
    }
    if (i === 23) {
      console.log("GANTT-TIMEOUT. URL:", page.url());
      console.log(
        "BODY:",
        JSON.stringify(
          (
            await page
              .locator("body")
              .innerText()
              .catch(() => "")
          ).slice(0, 500)
        )
      );
    }
  }
  await page.screenshot({ path: "/tmp/recon-118/gantt-reload.png" });
  console.log("VIS-DONE");
});
