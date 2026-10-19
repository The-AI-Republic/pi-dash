// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver billing methods (NEWFRONT-546,
// WSET-029–032). A stub Page stands in for Playwright: the assertions cover
// the address each method opens, the selector and read it performs, the
// click target it chooses, and the dialog absence it reports — never a real
// browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { billingPageSettled, WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  evaluate?: (fn: (node: Element) => unknown) => Promise<unknown>;
  evaluateAll?: (fn: (elements: Element[]) => unknown) => Promise<unknown>;
  allTextContents?: () => Promise<string[]>;
  textContent?: (options?: unknown) => Promise<string | null>;
  first?: () => StubLocator;
  filter?: (options: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: unknown, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 0,
    isVisible: async () => false,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    evaluate: async () => null,
    evaluateAll: async () => [],
    allTextContents: async () => [],
    textContent: async () => null,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    filter: () => node,
    getByRole: () => node,
    getByText: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    url: () => "https://app.test/acme/settings/billing",
    getByRole: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    keyboard: { press: vi.fn(async () => undefined) },
    waitForTimeout: vi.fn(async () => undefined),
    waitForEvent: vi.fn(async () => null),
    ...overrides,
  } as unknown as Page;
}

describe("billing driver", () => {
  it("opens the billing address and settles on the page or the denial view", async () => {
    const goto = vi.fn(async () => null);
    const waitForFunction = vi.fn(async () => undefined);
    const page = pageOf({ goto, waitForFunction });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsBillingOpen("acme");
    expect(goto).toHaveBeenCalledWith("/acme/settings/billing");
    expect(waitForFunction).toHaveBeenCalledTimes(1);
  });

  it("settles on either the plans body or the denial view", () => {
    const seen: string[] = [];
    const root = (found: string | null) => ({
      querySelector: (selector: string) => {
        seen.push(selector);
        return selector.includes(found ?? "\u0000") ? {} : null;
      },
    });
    expect(billingPageSettled(root("workspace-settings-billing"))).toBe(true);
    expect(billingPageSettled(root("workspace-settings-not-authorized"))).toBe(true);
    expect(billingPageSettled(root(null))).toBe(false);
    expect(seen).toContain('[data-testid="workspace-settings-billing"]');
  });

  it("reads the denial view from its own surface", async () => {
    const notAuthorized = locator({ count: async () => 1, isVisible: async () => true });
    const page = pageOf({ locator: vi.fn(() => notAuthorized) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingNotAuthorized()).toBe(true);
  });

  it("reads the community title and description", async () => {
    const page = pageOf({
      locator: vi.fn((selector: string) =>
        selector.includes("billing-community-description")
          ? locator({ count: async () => 1, innerText: async () => " Unlimited projects " })
          : locator({ count: async () => 1, innerText: async () => " Community " })
      ),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingCommunityTitle()).toBe("Community");
    expect(await driver.wsSettingsBillingCommunityDescription()).toBe("Unlimited projects");
  });

  it("lists the plan names and the popular plan from the card headings", async () => {
    const list = locator({
      evaluateAll: async (fn) =>
        fn(
          ["Pro", "Business", "Enterprise"].map(
            (name) => ({ querySelector: () => ({ textContent: name }) }) as unknown as Element
          )
        ),
    });
    const popular = locator({
      count: async () => 1,
      evaluate: async (fn) =>
        fn({
          closest: () => ({ querySelector: () => ({ textContent: "Pro" }) }),
        } as unknown as Element),
    });
    const page = pageOf({
      locator: vi.fn((selector: string) =>
        selector === '[data-testid="billing-plan-list"] [data-testid^="billing-plan-"]' ? list : popular
      ),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPlanNames()).toEqual(["Pro", "Business", "Enterprise"]);
    expect(await driver.wsSettingsBillingPopularPlan()).toBe("Pro");
  });

  it("reports no popular plan when no marker shows", async () => {
    const empty = locator({ count: async () => 0 });
    const page = pageOf({ locator: vi.fn(() => empty) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPopularPlan()).toBeNull();
  });

  it("reads a plan price, its discount and its action label", async () => {
    const action = locator({ count: async () => 1, innerText: async () => "Upgrade to Pro" });
    const page = pageOf({
      locator: vi.fn((selector: string) => {
        if (selector.includes("billing-price-pro"))
          return locator({ count: async () => 1, innerText: async () => "$8" });
        if (selector.includes("billing-discount-pro"))
          return locator({ count: async () => 1, innerText: async () => "-25%" });
        return locator({
          count: async () => 1,
          getAttribute: async () => "billing-plan-pro",
          locator: () => action,
        });
      }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPlanPrice("Pro")).toBe("$8");
    expect(await driver.wsSettingsBillingDiscountBadge("Pro")).toBe("-25%");
    expect(await driver.wsSettingsBillingPlanButtonLabel("Pro")).toBe("Upgrade to Pro");
  });

  it("returns null reads for a plan card that is not on the page", async () => {
    const page = pageOf({ locator: vi.fn(() => locator({ count: async () => 0 })) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPlanPrice("Pro")).toBeNull();
    expect(await driver.wsSettingsBillingFrequencyVisible("Pro")).toBe(false);
    expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBeNull();
    expect(await driver.wsSettingsBillingDiscountBadge("Pro")).toBeNull();
    expect(await driver.wsSettingsBillingPlanButtonLabel("Pro")).toBeNull();
    expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBeNull();
  });

  it("reports the selected frequency from the pressed half", async () => {
    const buttons = new Map<string, StubLocator>([
      ["Monthly", locator({ getAttribute: async () => "true" })],
      ["Yearly", locator({ getAttribute: async () => "false" })],
    ]);
    const card = locator({
      count: async () => 1,
      getByRole: (role: string, options: { name?: string } | undefined) =>
        buttons.get(String(options?.name ?? "")) ?? locator({ count: async () => 0 }),
    });
    const page = pageOf({ locator: vi.fn(() => card), getByRole: vi.fn(() => locator({ count: async () => 0 })) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBe("month");
  });

  it("reports no frequency when neither half is pressed", async () => {
    const card = locator({
      count: async () => 1,
      getByRole: () => locator({ getAttribute: async () => null }),
    });
    const page = pageOf({ locator: vi.fn(() => card) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBeNull();
  });

  it("clicks the frequency half for the requested choice", async () => {
    const yearly = locator({ click: vi.fn(async () => undefined) });
    const card = locator({
      count: async () => 1,
      getAttribute: async () => "year",
      getByRole: (role: string, options: { name?: string } | undefined) =>
        options?.name === "Yearly" ? yearly : locator({ count: async () => 0 }),
    });
    const page = pageOf({ locator: vi.fn(() => card), getByRole: vi.fn(() => locator({ count: async () => 0 })) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsBillingSelectFrequency("Pro", "year");
    expect(yearly.click).toHaveBeenCalledTimes(1);
  });

  it("reads the compare control label, its expanded state and the section titles", async () => {
    const toggle = locator({
      count: async () => 1,
      innerText: async () => "Collapse comparison",
      getAttribute: async (name: string) => (name === "aria-expanded" ? "true" : null),
    });
    const sections = locator({
      evaluateAll: async (fn) =>
        fn(
          ["Project + work tracking", "Teams and access"].map((title) => ({ textContent: title }) as unknown as Element)
        ),
    });
    const page = pageOf({
      locator: vi.fn((selector: string) => (selector.includes("billing-comparison-section-") ? sections : toggle)),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingCompareToggleLabel()).toBe("Collapse comparison");
    expect(await driver.wsSettingsBillingComparisonVisible()).toBe(true);
    expect(await driver.wsSettingsBillingComparisonSections()).toEqual(["Project + work tracking", "Teams and access"]);
  });

  it("flips the comparison control", async () => {
    const click = vi.fn(async () => undefined);
    const toggle = locator({ count: async () => 1, innerText: async () => "Compare all features", click });
    const page = pageOf({ locator: vi.fn(() => toggle) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsBillingToggleCompare();
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("returns no sections while the comparison is collapsed", async () => {
    const toggle = locator({
      count: async () => 1,
      getAttribute: async () => "false",
      innerText: async () => "Compare",
    });
    const page = pageOf({ locator: vi.fn(() => toggle) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingComparisonSections()).toEqual([]);
  });

  it("captures the popup target of a plan action without loading it", async () => {
    const popup = { url: () => "https://airepublic.com/pricing/pro?plan=month", close: vi.fn(async () => undefined) };
    const action = locator({ count: async () => 1 });
    const card = locator({ count: async () => 1, locator: () => action });
    const page = pageOf({
      locator: vi.fn(() => card),
      waitForEvent: vi.fn(async () => popup),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBe(
      "https://airepublic.com/pricing/pro?plan=month"
    );
    expect(popup.close).toHaveBeenCalledTimes(1);
  });

  it("returns null when the action opens no popup", async () => {
    const action = locator({ count: async () => 1 });
    const card = locator({ count: async () => 1, locator: () => action });
    const page = pageOf({ locator: vi.fn(() => card), waitForEvent: vi.fn(async () => null) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBeNull();
  });

  it("reports no dialog and no dialog title on any page", async () => {
    const empty = locator({ count: async () => 0 });
    const page = pageOf({ getByRole: vi.fn(() => empty) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsUpgradeModalVisible()).toBe(false);
    expect(await driver.wsSettingsUpgradeModalTitle()).toBeNull();
    await driver.wsSettingsUpgradeModalClose();
  });

  it("opens the workspace home and its user menu without a community entry", async () => {
    const goto = vi.fn(async () => null);
    const trigger = locator({ count: async () => 1, isVisible: async () => true, click: vi.fn(async () => undefined) });
    const menu = locator({
      count: async () => 1,
      textContent: async () => "Sign out",
      allTextContents: async () => ["Sign out"],
    });
    const page = pageOf({
      goto,
      locator: vi.fn((selector: string) => (selector.includes("sidebar-user-menu") ? trigger : menu)),
      getByRole: vi.fn((role: string) => (role === "menu" ? menu : locator({ count: async () => 0 }))),
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsUserMenuOpenCommunity("acme");
    // Settings addresses replace the app chrome (§1), so the menu comes
    // from the workspace home, exactly as the oracle reaches it.
    expect(goto).toHaveBeenCalledWith("/acme/");
    expect(trigger.click).toHaveBeenCalledTimes(1);
    expect(await driver.userMenuTexts()).toEqual(["Sign out", "Sign out"]);
  });

  it("opens the active-cycles address and returns the pro-pill count", async () => {
    const goto = vi.fn(async () => null);
    const pill = locator({ count: async () => 2, click: vi.fn(async () => undefined) });
    const page = pageOf({
      goto,
      locator: vi.fn((selector: string) => (selector.includes("upgrade-pill") ? pill : locator())),
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsActiveCyclesOpen("acme");
    expect(goto).toHaveBeenCalledWith("/acme/active-cycles");
    expect(await driver.wsSettingsProBadgeCount()).toBe(2);
    await driver.wsSettingsProBadgeClickFirst();
    expect(pill.click).toHaveBeenCalledTimes(1);
  });

  it("reports no popular plan when the marker read fails", async () => {
    const marker = locator({
      count: async () => 1,
      evaluate: async () => {
        throw new Error("detached");
      },
    });
    const page = pageOf({ locator: vi.fn(() => marker), getByText: vi.fn(() => marker) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPopularPlan()).toBeNull();
  });

  it("finds both switch halves on an initialized card", async () => {
    const card = locator({
      count: async () => 1,
      getByRole: () => locator({ count: async () => 1 }),
    });
    const page = pageOf({ locator: vi.fn(() => card) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingFrequencyVisible("Pro")).toBe(true);
  });

  it("reports a yearly selection from the pressed yearly half", async () => {
    const buttons = new Map<string, StubLocator>([
      ["Monthly", locator({ getAttribute: async () => "false" })],
      ["Yearly", locator({ getAttribute: async () => "true" })],
    ]);
    const card = locator({
      count: async () => 1,
      getByRole: (role: string, options: { name?: string } | undefined) =>
        buttons.get(String(options?.name ?? "")) ?? locator({ count: async () => 0 }),
    });
    const page = pageOf({ locator: vi.fn(() => card), getByRole: vi.fn(() => locator({ count: async () => 0 })) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingSelectedFrequency("Pro")).toBe("year");
  });

  it("clicks the monthly half when month is requested", async () => {
    const monthly = locator({ click: vi.fn(async () => undefined) });
    const card = locator({
      count: async () => 1,
      getAttribute: async () => "month",
      getByRole: (role: string, options: { name?: string } | undefined) =>
        options?.name === "Monthly" ? monthly : locator({ count: async () => 0 }),
    });
    const page = pageOf({ locator: vi.fn(() => card) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsBillingSelectFrequency("Pro", "month");
    expect(monthly.click).toHaveBeenCalledTimes(1);
  });

  it("reports no sections and an unexpanded comparison when the toggle read fails", async () => {
    const toggle = locator({
      count: async () => 1,
      getAttribute: async () => {
        throw new Error("detached");
      },
    });
    const page = pageOf({ locator: vi.fn(() => toggle) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingComparisonVisible()).toBe(false);
    expect(await driver.wsSettingsBillingComparisonSections()).toEqual([]);
  });

  it("flips the comparison without waiting when the label never read", async () => {
    const click = vi.fn(async () => undefined);
    const toggle = locator({ count: async () => 0, click });
    const page = pageOf({ locator: vi.fn(() => toggle) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsBillingToggleCompare();
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("drops blank comparison section headings", async () => {
    const toggle = locator({
      count: async () => 1,
      getAttribute: async () => "true",
    });
    const sections = locator({
      evaluateAll: async (fn) =>
        fn(
          [{ textContent: " Project + work tracking " }, { textContent: "  " }].map(
            (entry) => entry as unknown as Element
          )
        ),
    });
    const page = pageOf({
      locator: vi.fn((selector: string) => (selector.includes("billing-comparison-section-") ? sections : toggle)),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingComparisonSections()).toEqual(["Project + work tracking"]);
  });

  it("returns null when the popup never leaves about:blank", async () => {
    const popup = { url: () => "about:blank", close: vi.fn(async () => undefined) };
    const action = locator({ count: async () => 1 });
    const card = locator({ count: async () => 1, locator: () => action });
    let clock = 0;
    const realNow = Date.now;
    vi.spyOn(Date, "now").mockImplementation(() => {
      clock += 7_500;
      return clock;
    });
    try {
      const page = pageOf({
        locator: vi.fn(() => card),
        waitForEvent: vi.fn(async () => popup),
        waitForTimeout: vi.fn(async () => undefined),
      });
      const driver = new WebNewDriver(page);
      expect(await driver.wsSettingsBillingPlanButtonPopupUrl("Pro")).toBeNull();
      expect(popup.close).toHaveBeenCalledTimes(1);
    } finally {
      vi.spyOn(Date, "now").mockRestore();
      void realNow;
    }
  });

  it("reads any dialog, not just the old modal copy, and its title", async () => {
    const heading = locator({ count: async () => 1, innerText: async () => "Pick a plan" });
    const dialog = locator({
      count: async () => 1,
      isVisible: async () => true,
      getByRole: () => heading,
      filter: () => dialog,
    });
    const page = pageOf({ getByRole: vi.fn(() => dialog), keyboard: { press: vi.fn(async () => undefined) } });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsUpgradeModalVisible()).toBe(true);
    expect(await driver.wsSettingsUpgradeModalTitle()).toBe("Pick a plan");
  });

  it("sends Escape to a stray dialog and tolerates it staying", async () => {
    const visible = { value: true };
    const dialog = locator({
      count: async () => 1,
      isVisible: async () => visible.value,
    });
    const page = pageOf({ getByRole: vi.fn(() => dialog), keyboard: { press: vi.fn(async () => undefined) } });
    const driver = new WebNewDriver(page);
    const press = vi.fn(async () => {
      visible.value = false;
    });
    (page.keyboard as unknown as { press: typeof press }).press = press;
    await driver.wsSettingsUpgradeModalClose();
    expect(press).toHaveBeenCalledWith("Escape");
  });

  it("fails loudly when the account menu never opens", async () => {
    const trigger = locator({ click: vi.fn(async () => undefined) });
    const menu = locator({ count: async () => 0 });
    const page = pageOf({
      locator: vi.fn((selector: string) => (selector.includes("sidebar-user-menu") ? trigger : menu)),
      getByRole: vi.fn(() => menu),
    });
    const driver = new WebNewDriver(page);
    // First read sets the deadline, the poll then sees it passed.
    const now = vi.spyOn(Date, "now").mockReturnValueOnce(0).mockReturnValue(60_000);
    try {
      await expect(driver.wsSettingsUserMenuOpenCommunity("acme")).rejects.toThrow(/user menu never opened/);
    } finally {
      now.mockRestore();
    }
    expect(trigger.click).toHaveBeenCalledTimes(1);
  });

  it("does nothing when there is no pro pill to click", async () => {
    const pill = locator({ count: async () => 0, click: vi.fn(async () => undefined) });
    const page = pageOf({ locator: vi.fn(() => pill) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsProBadgeClickFirst();
    expect(pill.click).not.toHaveBeenCalled();
  });

  it("reads the full page text collapsed to one line", async () => {
    const body = locator({ innerText: async () => "  Community \n All plans " });
    const page = pageOf({ locator: vi.fn((selector: string) => (selector === "body" ? body : locator())) });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsBillingPageText()).toBe("Community All plans");
  });
});
