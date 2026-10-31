// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): workspace plan badge, row CLOUD-038
// (no plan badge renders in the sidebar), green on the cloud-edition old
// app first. The same file must go green on apps/web_new.
//
// Observed behavior notes (cloud oracle): no plan badge renders — the
// sidebar restructure dropped the badge mount and the cloud override
// was never re-mounted, so it is dead code (bug NEWFRONT-474). What
// cloud users get instead is the OSS user-menu "Community" entry with
// its paid-plans modal. The plan API itself survives and still serves
// the snapshot the badge was built on.
//
// Owner decision (NEWFRONT-1045, superseding NEWFRONT-474's intended
// badge): the new frontend renders no badge either, in any plan state.
// The sidebar footer holds only the user menu, which spans the full
// width; the running version still surfaces in the help menu and the
// product-updates dialog. The scenarios below pin the absence on both
// targets plus the surviving plan API — not the removed badge.
import { test, expect } from "../fixtures";
import { cloudBillingPlanRaw, cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudWorkspaceUser } from "./cloud-user-setup";

test(
  specTitle(
    ["CLOUD-038"],
    "no plan badge renders in the sidebar (NEWFRONT-1045 removed the badge; the plan API still serves)"
  ),
  { tag: specTags(["CLOUD-038"]) },
  async ({ driver, seed }) => {
    const member = await cloudWorkspaceUser(seed, "parity-440-badge", 15, "Badge Absent");
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(member.cookie)
    );
    await expect.poll(() => driver.sidebarEntryVisible("Projects"), { timeout: 60_000 }).toBe(true);
    // Both apps render no badge: the old app never mounted its override
    // (NEWFRONT-474), and the new frontend removed its badge by owner
    // decision (NEWFRONT-1045).
    expect(await driver.cloudPlanBadgeVisible()).toBe(false);

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
    "user menu carries no Community entry on the new frontend (NEWFRONT-1045; the old app keeps the OSS entry)"
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
      // NEWFRONT-1045: neither the plan badge nor the OSS Community entry.
      expect(await driver.cloudPlanBadgeVisible()).toBe(false);
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
