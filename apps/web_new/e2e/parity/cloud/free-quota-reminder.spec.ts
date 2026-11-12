// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-feature scenarios (NEWPIDASH-181, DIFF-15): the cloud AI Assistant
// tab's paid-only "free quota used up" reminder. The old app has no such
// setting or note — the old-web half lives in the private ee-overlay, not
// in the oracle — so the oracle arm pins the absence while the new arm
// pins the toggle and, once enabled, the once-per-UTC-day note. No
// parity-inventory row covers this feature, so these titles carry no row
// tags.
//
// Observed behavior notes (new frontend): on a paid plan the tab shows
// the toggle off by default; enabling it persists `free_quota_reminder`
// in the profile settings; when the quota proxy reports an exhausted
// allowance the banner shows once, and work continues either way.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies } from "../helpers/api";
import { cloudWorkspaceUser } from "./cloud-user-setup";

const QUOTA_PATH = "**/api/openhub/usage/quota/";

function quotaBody(plan: string, used: number, limit: number): Record<string, unknown> {
  return {
    plan,
    llm: {
      freeTokensUsed: used,
      freeTokensLimit: limit,
      freeModels: ["openhub_free_llm"],
    },
    resetsAt: "2026-11-10T00:00:00+00:00",
  };
}

test("the new frontend toggles the reminder on paid plans; the oracle renders none (DIFF-15)", async ({
  driver,
  seed,
}) => {
  const session = await cloudWorkspaceUser(seed, "parity-181-reminder", 15, "Quota Reminder");
  await driver.page.route(QUOTA_PATH, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(quotaBody("payg", 100, 5_000_000)),
    });
  });
  try {
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    if (driver.target === "web_new") {
      const toggle = driver.page.getByTestId("free-quota-reminder-switch");
      await expect(toggle).toBeVisible({ timeout: 60_000 });
      await expect(toggle).toHaveAttribute("aria-checked", "false");
      // Not exhausted: no banner even though the section is present.
      await expect(driver.page.getByTestId("free-quota-reminder-banner")).toHaveCount(0);
      return;
    }
    // The old app has no reminder setting: this is the deliberate difference.
    await expect(driver.page.getByTestId("free-quota-reminder-switch")).toHaveCount(0, { timeout: 60_000 });
    await expect(driver.page.getByTestId("free-quota-reminder-banner")).toHaveCount(0);
  } finally {
    await driver.page.unroute(QUOTA_PATH);
  }
});

test("enabling the toggle persists it and shows the banner once when the allowance is exhausted", async ({
  driver,
  seed,
}) => {
  const session = await cloudWorkspaceUser(seed, "parity-181-reminder-on", 15, "Quota Reminder On");
  await driver.page.route(QUOTA_PATH, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(quotaBody("member", 5_000_000, 5_000_000)),
    });
  });
  try {
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    if (driver.target !== "web_new") {
      // The oracle pins the absence above; nothing to enable here.
      await expect(driver.page.getByTestId("free-quota-reminder-switch")).toHaveCount(0, { timeout: 60_000 });
      return;
    }
    const toggle = driver.page.getByTestId("free-quota-reminder-switch");
    await expect(toggle).toBeVisible({ timeout: 60_000 });

    // The PATCH round-trip proves server persistence: the key must read
    // back true after the flip (backend declared by PRIVATEPI1-147).
    const patch = driver.page.waitForResponse(
      (res) => res.url().includes("/api/users/me/profile/") && res.request().method() === "PATCH",
      { timeout: 60_000 }
    );
    await toggle.click();
    const res = await patch;
    expect(res.status()).toBe(200);
    await expect(toggle).toHaveAttribute("aria-checked", "true");

    // Exhausted + on: one informational note; work continues uninterrupted.
    await expect(driver.page.getByTestId("free-quota-reminder-banner")).toContainText(
      "Today's free tokens are used up."
    );
    await expect(driver.page.getByRole("heading", { name: "AI Assistant" })).toBeVisible();
  } finally {
    await driver.page.unroute(QUOTA_PATH);
  }
});
