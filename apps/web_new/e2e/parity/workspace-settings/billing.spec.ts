// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-414): workspace billing and plans. Rows
// WSET-029 (billing page: community-plan summary plus plan comparison
// with a monthly/yearly switch), WSET-030 (plan actions open external
// pricing/sales pages), WSET-031 (negative: no paid-plan upgrade dialog;
// the sidebar user-menu community entry still opens one, bug
// NEWFRONT-423), WSET-032 (OSS: community plan only, no portal, trial
// or in-app checkout controls).
//
// Observed behavior notes (this OSS checkout, self-managed billing
// root): the billing page is admin-only and everyone else gets the
// not-authorized view; the page shows a community summary box plus an
// "All plans" comparison of Pro, Business and Enterprise (Free and One
// never render), Pro carrying the popularity marker; every card shows
// a Monthly/Yearly switch defaulting to monthly, only Pro showing a
// discount badge; an expander reveals the full feature comparison.
// The Pro card action opens the external per-frequency pricing page in
// a new tab (Business lands on its pricing page despite the "Talk to
// Sales" label; Enterprise lands on the sales contact page); nothing is
// purchased in-app and no subscription state changes. The upgrade pills
// beside pro-gated features are
// static labels that open nothing, and the edition badge component has
// no mount point; only the user-menu community entry still opens the
// upgrade dialog (bug NEWFRONT-423, intended removed).
import { test, expect } from "../fixtures";
import { serverCleanupWorkspaceMember, serverWorkspaceMembers } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  isCloudEdition,
  markTourCompleted,
  openWorkspacePage,
  provisionMember as provisionEditionMember,
  seededSession,
  type EditionUser,
} from "./edition-session";

/** The seeded workspace owner, entered in this edition's way. */
async function ownerUser(seed: ParitySeedFacts): Promise<EditionUser> {
  return { email: seed.email, session: await seededSession(seed, seed.email), password: seed.password };
}

/** Workspace membership id for cleanup, resolved by provisioned user id. */
async function membershipIdFor(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<string> {
  const rows = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
  const row = rows.find((entry) => entry.userId === userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  return row.membershipId;
}

test(
  specTitle(["WSET-029"], "billing page shows community summary, plan comparison and frequency switch"),
  { tag: specTags(["WSET-029"]) },
  async ({ driver, seed }) => {
    const owner = await ownerUser(seed);
    const ownerSession = owner.session;
    await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, owner);

    await test.step("admin sees the community summary and three plan cards", async () => {
      await driver.wsSettingsBillingOpen(seed.workspaceSlug);
      expect(await driver.wsSettingsBillingNotAuthorized()).toBe(false);
      expect(await driver.wsSettingsBillingCommunityTitle()).toBe("Community");
      expect(await driver.wsSettingsBillingCommunityDescription()).toContain("Unlimited projects");
      expect(await driver.wsSettingsBillingPlanNames()).toEqual(["Pro", "Business", "Enterprise"]);
      expect(await driver.wsSettingsBillingPopularPlan()).toBe("Pro");
    });

    await test.step("frequency switch defaults to monthly and reprices the Pro card", async () => {
      expect(await driver.wsSettingsBillingFrequencyVisible("Pro")).toBe(true);
      expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBe("month");
      expect(await driver.wsSettingsBillingPlanPrice("Pro")).toBe("$8");
      expect(await driver.wsSettingsBillingDiscountBadge("Pro")).toBe("-25%");
      await driver.wsSettingsBillingSelectFrequency("Pro", "year");
      expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBe("year");
      expect(await driver.wsSettingsBillingPlanPrice("Pro")).toBe("$6");
      await driver.wsSettingsBillingSelectFrequency("Pro", "month");
      expect(await driver.wsSettingsBillingPlanPrice("Pro")).toBe("$8");
    });

    await test.step("inactive cards show the switch with a quote line and no discount", async () => {
      for (const plan of ["Business", "Enterprise"]) {
        expect(await driver.wsSettingsBillingFrequencyVisible(plan)).toBe(true);
        expect(await driver.wsSettingsBillingPlanPrice(plan)).toBe("Quote on request");
        expect(await driver.wsSettingsBillingDiscountBadge(plan)).toBeNull();
      }
    });

    await test.step("full comparison expands and collapses with section titles", async () => {
      expect(await driver.wsSettingsBillingCompareToggleLabel()).toBe("Compare all features");
      expect(await driver.wsSettingsBillingComparisonVisible()).toBe(false);
      expect(await driver.wsSettingsBillingComparisonSections()).toEqual([]);
      await driver.wsSettingsBillingToggleCompare();
      expect(await driver.wsSettingsBillingCompareToggleLabel()).toBe("Collapse comparison");
      expect(await driver.wsSettingsBillingComparisonVisible()).toBe(true);
      const sections = await driver.wsSettingsBillingComparisonSections();
      expect(sections).toContain("Project + work tracking");
      expect(sections.length).toBeGreaterThan(1);
      await driver.wsSettingsBillingToggleCompare();
      expect(await driver.wsSettingsBillingComparisonVisible()).toBe(false);
    });

    await test.step("non-admin member gets the not-authorized view", async () => {
      const member = await provisionEditionMember(seed, ownerSession, "wset029-member", 15);
      try {
        await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, member);
        await driver.wsSettingsBillingOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsBillingNotAuthorized()).toBe(true);
      } finally {
        await serverCleanupWorkspaceMember(
          seed.workspaceSlug,
          await membershipIdFor(seed, ownerSession, member.userId),
          ownerSession
        );
      }
    });
  }
);

test(
  specTitle(["WSET-030"], "plan actions open external pricing and sales pages"),
  { tag: specTags(["WSET-030"]) },
  async ({ driver, seed }) => {
    await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, await ownerUser(seed));
    await driver.wsSettingsBillingOpen(seed.workspaceSlug);

    await test.step("Pro action opens the per-frequency pricing page", async () => {
      expect(await driver.wsSettingsBillingPlanButtonLabel("Pro")).toBe("Upgrade to Pro");
      expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBe("month");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBe(
        "https://airepublic.com/pricing/pro?plan=month"
      );
      await driver.wsSettingsBillingSelectFrequency("Pro", "year");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBe(
        "https://airepublic.com/pricing/pro?plan=year"
      );
      await driver.wsSettingsBillingSelectFrequency("Pro", "month");
    });

    await test.step("Business action opens the per-frequency pricing page", async () => {
      // Observed quirk: the button reads "Talk to Sales" but lands on the
      // Business pricing page, not the sales contact page.
      expect(await driver.wsSettingsBillingPlanButtonLabel("Business")).toBe("Talk to Sales");
      expect(await driver.wsSettingsBillingSelectedFrequency("Business")).toBe("month");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Business")).toBe(
        "https://airepublic.com/pricing/business?plan=month"
      );
      await driver.wsSettingsBillingSelectFrequency("Business", "year");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Business")).toBe(
        "https://airepublic.com/pricing/business?plan=year"
      );
      await driver.wsSettingsBillingSelectFrequency("Business", "month");
    });

    await test.step("Enterprise action opens the sales contact page", async () => {
      expect(await driver.wsSettingsBillingPlanButtonLabel("Enterprise")).toBe("Talk to Sales");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Enterprise")).toBe("https://airepublic.com/contact");
      await driver.wsSettingsBillingSelectFrequency("Enterprise", "year");
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Enterprise")).toBe("https://airepublic.com/contact");
      await driver.wsSettingsBillingSelectFrequency("Enterprise", "month");
    });

    await test.step("opening checkout targets changes nothing in-app", async () => {
      // The actions open external pages in a new tab: the billing page
      // itself stays put with no dialog, no notice and no plan change.
      expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
      expect(await driver.wsSettingsBillingPlanNames()).toEqual(["Pro", "Business", "Enterprise"]);
      expect(await driver.wsSettingsBillingNotAuthorized()).toBe(false);
    });
  }
);

test(
  specTitle(["WSET-031"], "no upgrade dialog opens at former trigger points"),
  { tag: specTags(["WSET-031"]) },
  async ({ driver, seed }) => {
    await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, await ownerUser(seed));

    await test.step("billing page shows no upgrade dialog", async () => {
      await driver.wsSettingsBillingOpen(seed.workspaceSlug);
      expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
    });

    await test.step("upgrade pills are static labels that open nothing", async () => {
      if (driver.target === "web_new") {
        // The active-cycles route belongs to the cycles area (CYC-043), so
        // the pill's mount host has not landed yet. The rest of this row
        // (no dialog at the former trigger points, no plan surface in the
        // shell) is proven above; the pill leg skips like the shell's
        // SHELL-101 precedent until the host exists. TODO(NEWFRONT-527).
        test.skip(true, "cycles route unlanded; pill mount unprovable");
        return;
      }
      // The active-cycles header carries an upgrade pill.
      await driver.wsSettingsActiveCyclesOpen(seed.workspaceSlug);
      expect(await driver.wsSettingsProBadgeCount()).toBeGreaterThan(0);
      await driver.wsSettingsProBadgeClickFirst();
      expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["WSET-031"], "bug: user-menu community entry still opens the paid-plan upgrade dialog"),
  { tag: specTags(["WSET-031"]) },
  async ({ driver, seed }) => {
    // Bug NEWFRONT-423: the upgrade dialog was intended removed (WSET-031
    // negative row), but the sidebar user-menu community entry still
    // opens it. Pinned as observed until the removal lands. web_new builds
    // the intended target (§31): no community entry, so no dialog.
    // The first-run tour overlay would cover the home sidebar, so the
    // seeded tour flag is completed first (home-spec precedent).
    const owner = await ownerUser(seed);
    await markTourCompleted(owner.session);
    await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, owner);
    await driver.wsSettingsBillingOpen(seed.workspaceSlug);
    expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
    await driver.wsSettingsUserMenuOpenCommunity(seed.workspaceSlug);
    if (driver.target === "web_new") {
      // Intended: the entry is gone, so activating the menu opens nothing.
      expect(await driver.userMenuTexts()).not.toEqual([]);
      expect((await driver.userMenuTexts()).join(" ")).not.toContain("Community");
      expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
      return;
    }
    expect(await driver.wsSettingsUpgradeModalVisible()).toBe(true);
    expect(await driver.wsSettingsUpgradeModalTitle()).toBe("Upgrade to a paid plan and unlock missing features.");
    await driver.wsSettingsUpgradeModalClose();
    expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
  }
);

test(
  specTitle(["WSET-032"], "OSS shows the community plan with no portal, trial or checkout controls"),
  { tag: specTags(["WSET-032"]) },
  async ({ driver, seed }) => {
    // OSS-tagged row (§32): the absence it pins describes the community
    // build. A cloud build carries its own billing surface (CLOUD rows),
    // so this scenario is not part of the cloud leg.
    test.skip(isCloudEdition(), "WSET-032 is an OSS-only row");
    await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/billing`, await ownerUser(seed));
    await driver.wsSettingsBillingOpen(seed.workspaceSlug);

    await test.step("community summary renders without subscription controls", async () => {
      expect(await driver.wsSettingsBillingNotAuthorized()).toBe(false);
      expect(await driver.wsSettingsBillingCommunityTitle()).toBe("Community");
      const text = await driver.wsSettingsBillingPageText();
      expect(text).not.toMatch(/trial/i);
      expect(text).not.toMatch(/portal/i);
      expect(text).not.toMatch(/checkout/i);
      expect(text).not.toMatch(/current plan/i);
      expect(text).not.toMatch(/cancel subscription/i);
    });

    await test.step("plan actions leave the app instead of purchasing in-app", async () => {
      // Every plan action opens an external page: there is no in-app
      // purchase, subscription or payment surface on this build.
      for (const plan of ["Pro", "Business", "Enterprise"]) {
        const target = await driver.wsSettingsBillingPlanButtonPopupUrl(plan);
        expect(target).toMatch(/^https:\/\/airepublic\.com\//);
      }
      expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
    });
  }
);
