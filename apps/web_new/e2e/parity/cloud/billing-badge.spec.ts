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
    await driver.openUserMenu();
    await expect.poll(() => driver.userMenuTexts(), { timeout: 60_000 }).toContain("Community");
    await driver.activateUserMenuItem("Community");
    await expect.poll(() => driver.page.getByRole("dialog").count(), { timeout: 60_000 }).toBeGreaterThan(0);
  }
);
