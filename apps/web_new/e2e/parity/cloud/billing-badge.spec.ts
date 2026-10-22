// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): workspace plan badge, row CLOUD-038
// (the badge shows the cloud plan and links out for billing), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): no plan badge renders — the
// sidebar restructure dropped the badge mount and the cloud override
// was never re-mounted, so it is dead code (bug NEWFRONT-474). What
// cloud users get instead is the OSS user-menu "Community" entry with
// its paid-plans modal. The plan API itself survives and still serves
// the snapshot the badge was built on. The new frontend must implement
// the intended badge — plan label, account-center link, version hint,
// focus refresh, failure-keeps-label — not this absence.
import { test, expect } from "../fixtures";
import { cloudBillingPlanRaw, cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudWorkspaceUser } from "./cloud-user-setup";

test(
  specTitle(
    ["CLOUD-038"],
    "bug: NEWFRONT-474 no plan badge renders in the sidebar (intended: the plan label with its account-center link)"
  ),
  { tag: specTags(["CLOUD-038"]) },
  async ({ driver, seed }) => {
    const member = await cloudWorkspaceUser(seed, "parity-440-badge", 15, "Badge Absent");
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(member.cookie)
    );
    await expect.poll(() => driver.sidebarEntryVisible("Projects"), { timeout: 60_000 }).toBe(true);
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-474): the sidebar badge labels the plan and
      // opens the account center; the hover hint carries the version.
      await expect.poll(() => driver.cloudPlanBadgeVisible(), { timeout: 60_000 }).toBe(true);
      const badge = await driver.cloudPlanBadgeFacts();
      expect(badge?.label).toBe("Upgrade");
      expect(badge?.hint).toContain("Version:");
      const center = (await cloudBillingPlanRaw(member.cookie)).payload["user_center_url"] as string;
      await driver.page.context().route(center, async (route) => {
        await route.fulfill({ status: 200, contentType: "text/html", body: "<title>center</title>" });
      });
      try {
        const popupWait = driver.page.waitForEvent("popup", { timeout: 60_000 });
        await driver.page.getByTestId("cloud-plan-badge").click();
        const popup = await popupWait;
        await popup.waitForLoadState("domcontentloaded", { timeout: 60_000 }).catch(() => undefined);
        expect(popup.url()).toBe(center);
        await popup.close();
      } finally {
        await driver.page.context().unroute(center);
      }
      // Focus refreshes the read; a failed refresh keeps the label. Real
      // tab switches do not move the focus manager under headless, so
      // drive the app's shared instance directly (the slice's own seam):
      // the exact dep URL matters, a bare re-import forks the module.
      const refocus = () =>
        driver.page.evaluate(async () => {
          const urls = performance.getEntriesByType("resource").map((entry) => entry.name);
          const dep = urls.find((url) => url.includes("@tanstack_react-query"));
          if (dep === undefined) throw new Error("[parity] react-query dep is not loaded.");
          const rq = (await import(dep)) as { focusManager: { setFocused(focused: boolean): void } };
          rq.focusManager.setFocused(false);
          rq.focusManager.setFocused(true);
        });
      let reads = 0;
      await driver.page.route("**/api/billing/me/plan/", async (route) => {
        reads += 1;
        await route.continue();
      });
      try {
        await refocus();
        await expect.poll(() => reads, { timeout: 60_000 }).toBeGreaterThan(0);
      } finally {
        await driver.page.unroute("**/api/billing/me/plan/");
      }
      let failures = 0;
      await driver.page.route("**/api/billing/me/plan/", async (route) => {
        failures += 1;
        await route.fulfill({ status: 500, body: "{}" });
      });
      try {
        await refocus();
        await expect.poll(() => failures, { timeout: 60_000 }).toBeGreaterThan(0);
        expect((await driver.cloudPlanBadgeFacts())?.label).toBe("Upgrade");
      } finally {
        await driver.page.unroute("**/api/billing/me/plan/");
      }
    } else {
      expect(await driver.cloudPlanBadgeVisible()).toBe(false);
    }

    // Server state: the plan snapshot the badge was built on still serves.
    const plan = await cloudBillingPlanRaw(member.cookie);
    expect(plan.status).toBe(200);
    expect(plan.payload["is_paid"]).toBe(false);
    expect(plan.payload["migration_required"]).toBe(false);
    const center = plan.payload["user_center_url"];
    expect(typeof center).toBe("string");
    expect((center as string).length).toBeGreaterThan(0);
  }
);

test(
  specTitle(
    ["CLOUD-038"],
    "bug: NEWFRONT-474 user menu carries the OSS Community entry instead (intended: the plan badge)"
  ),
  { tag: specTags(["CLOUD-038"]) },
  async ({ driver, seed }) => {
    const member = await cloudWorkspaceUser(seed, "parity-440-badge", 15, "Badge Menu");
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(member.cookie)
    );
    await expect.poll(() => driver.sidebarEntryVisible("Projects"), { timeout: 60_000 }).toBe(true);
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-474): the plan badge, not the OSS Community entry.
      await expect.poll(() => driver.cloudPlanBadgeVisible(), { timeout: 60_000 }).toBe(true);
      await driver.openUserMenu();
      expect(await driver.userMenuTexts()).not.toContain("Community");
      return;
    }
    await driver.openUserMenu();
    await expect.poll(() => driver.userMenuTexts(), { timeout: 60_000 }).toContain("Community");
    await driver.activateUserMenuItem("Community");
    await expect.poll(() => driver.page.getByRole("dialog").count(), { timeout: 60_000 }).toBeGreaterThan(0);
  }
);
