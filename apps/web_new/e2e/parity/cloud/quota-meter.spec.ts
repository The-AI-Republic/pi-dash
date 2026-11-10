// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-feature scenarios (NEWPIDASH-149, DIFF-8): the AI Assistant tab's
// free-token usage meter, reading the quota proxy at
// `GET /api/openhub/usage/quota/` (PRIVATEPI1-137). The old app has no
// meter — the old-web half lives in the private ee-overlay, not in the
// oracle — so the oracle arm pins the absence while the new arm pins
// the sentence. No parity-inventory row covers this feature (the
// inventory lists pre-feature rows only), so these titles carry no row
// tags.
//
// Observed behavior notes (new frontend): the meter sits at the top of
// the list view, states "X of Y free tokens used today, resets at
// HH:MM (local)" from the LLM allowance and the reset instant, and
// renders nothing while the quota loads or when the read fails — the
// tab stays usable either way.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies } from "../helpers/api";
import { cloudWorkspaceUser } from "./cloud-user-setup";

const QUOTA_PATH = "**/api/openhub/usage/quota/";

const QUOTA_BODY = {
  plan: "free",
  llm: {
    freeTokensUsed: 123_456,
    freeTokensLimit: 2_000_000,
    freeModels: ["deepseek/deepseek-v4.1-flash-1017"],
  },
  apps: { freeCallsUsed: 5, freeCallsLimit: 150 },
  resetsAt: "2026-11-10T00:00:00+00:00",
};

test("the new frontend shows the free-token meter; the oracle renders none (DIFF-8)", async ({ driver, seed }) => {
  const session = await cloudWorkspaceUser(seed, "parity-149-quota", 15, "Quota Meter");
  await driver.page.route(QUOTA_PATH, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(QUOTA_BODY),
    });
  });
  try {
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    if (driver.target === "web_new") {
      const meter = driver.page.getByTestId("free-token-meter");
      await expect(meter).toBeVisible({ timeout: 60_000 });
      await expect(meter).toHaveText(/123\.5K of 2M free tokens used today, resets at \d{2}:\d{2} \(local\)/);
      return;
    }
    // The old app has no meter: this is the deliberate difference.
    await expect(driver.page.getByTestId("free-token-meter")).toHaveCount(0, { timeout: 60_000 });
  } finally {
    await driver.page.unroute(QUOTA_PATH);
  }
});

test("a failed quota read hides the meter and leaves the tab usable", async ({ driver, seed }) => {
  const session = await cloudWorkspaceUser(seed, "parity-149-quota-hide", 15, "Quota Hidden");
  await driver.page.route(QUOTA_PATH, async (route) => {
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ error: "openhub_unavailable", detail: "OpenHub is unavailable right now." }),
    });
  });
  try {
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect(driver.page.getByRole("heading", { name: "AI Assistant" })).toBeVisible({ timeout: 60_000 });
    await expect(driver.page.getByTestId("free-token-meter")).toHaveCount(0);
  } finally {
    await driver.page.unroute(QUOTA_PATH);
  }
});
