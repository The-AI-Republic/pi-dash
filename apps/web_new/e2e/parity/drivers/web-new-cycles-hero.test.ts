// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycles hero methods (NEWFRONT-716 gate,
// CYC-040–CYC-045, CYC-047–CYC-048): the hero cards (progress, burn-down,
// breakdown) and their clicks, the no-active empty view, the workspace
// upsell, the timezone offset chips and tooltips, the traffic spy, the
// shared create dialog, the row transfer action, and the guest read-only
// matrix (search, filters, peek, row menu, clipboard). A recording fake
// stands in for Playwright: each locator remembers the path of calls that
// built it, scripted answers stand in for the page, and a test asserts
// both which selector the driver used and what it does with the answer.
// Never about a real browser.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "locator", "filter"];
const ACTIONS = [
  "count",
  "click",
  "hover",
  "fill",
  "innerText",
  "inputValue",
  "allTextContents",
  "allInnerTexts",
  "getAttribute",
  "waitFor",
  "isVisible",
  "isDisabled",
];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "function") return "<fn>";
      if (arg instanceof RegExp) return String(arg);
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") return JSON.stringify(arg);
      return String(arg);
    })
    .join(",");
}

const HERO_PATH = `page.getByTestId("cycle-hero")`;
const HEADING_PATH = `${HERO_PATH}.getByRole("button").first()`;
const ACTIVE_NAME_PATH = `${HERO_PATH}.locator("a[href*=\\"/cycles/\\"] span.truncate").first()`;
const CARDS_PATH = `page.getByTestId("cycle-hero-cards")`;
const NO_ACTIVE_PATH = `page.getByTestId("no-active-cycle")`;
const PROGRESS_CARD_PATH = `page.getByTestId("hero-progress-card")`;
const CLOSED_PATH = `${PROGRESS_CARD_PATH}.getByTestId("hero-progress-closed")`;
const GROUP_ROWS_PATH = `${PROGRESS_CARD_PATH}.locator("button[data-testid=\\"hero-progress-group\\"]")`;
const BURNDOWN_CARD_PATH = `page.getByTestId("hero-burndown-card")`;
const BREAKDOWN_CARD_PATH = `page.getByTestId("hero-breakdown-card")`;
const UPSELL_PATH = `page.getByTestId("workspace-actives-upsell")`;
const BENEFITS_PATH = `${UPSELL_PATH}.getByTestId("workspace-actives-benefits")`;
const TOOLTIP_PATH = `page.getByRole("tooltip").first()`;
const TOAST_PATH = `page.locator("[data-testid=\\"toast-notification\\"]")`;
const SEARCH_TOGGLE_PATH = `page.getByTestId("cycles-search-toggle")`;
const SEARCH_BOX_PATH = `page.getByTestId("cycles-search-box")`;
const SEARCH_INPUT_PATH = `page.getByRole("textbox",{"name":"Search cycles"})`;
const FILTER_TRIGGER_PATH = `page.getByTestId("cycles-filters-trigger")`;
const FILTER_PANEL_PATH = `page.getByTestId("cycles-filters-panel")`;
const SIDEBAR_DATES_PATH = `page.getByTestId("cycle-sidebar-dates")`;
const SIDEBAR_TRIGGER_PATH = `page.getByTestId("cycle-sidebar-dates-trigger").first()`;
const TRANSFER_LISTBOX_PATH = `page.getByRole("listbox",{"name":"Cycles"})`;
const TRANSFER_OPTIONS_PATH = `${TRANSFER_LISTBOX_PATH}.getByRole("option")`;
const TRANSFER_NAME_SPANS_PATH = `${TRANSFER_OPTIONS_PATH}.locator(":scope > span.truncate")`;

function rowPath(name: string): string {
  return `${rowFilterPath(name)}.first()`;
}

function rowFilterPath(name: string): string {
  return `page.locator("div[data-testid=\\"cycle-row\\"]").filter({"hasText":${JSON.stringify(name)}})`;
}

function nthIndex(path: string): number | null {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match === null ? null : Number(match[1]);
}

interface ScriptedRoute {
  predicate: unknown;
  handler: (route: Route) => Promise<void>;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly gotoUrls: string[] = [];
  readonly waitedUrls: unknown[] = [];
  readonly keyboardPresses: string[] = [];
  readonly fills: { path: string; value: string }[] = [];
  readonly routes: ScriptedRoute[] = [];
  readonly grantedPermissions: string[][] = [];
  readonly evaluated: unknown[] = [];
  evaluateResult: unknown = undefined;
  headingCount = 1;
  headingText = "Active cycle 1";
  activeNameCount = 1;
  activeNameVisible = true;
  activeNameText = "Hero cycle";
  heroCardsCount = 1;
  noActiveCount = 0;
  emptyTitle = "No active cycle";
  emptyDescription = "No cycle covers today.";
  progressClosedCount = 1;
  progressClosedText = "0/2 Work items closed";
  progressGroupNames: string[] = ["Started", "Backlog"];
  progressGroupTexts: string[] = ["Started 1", "Backlog 1"];
  progressEmptyCount = 0;
  progressEmptyVisible = false;
  burndownHeading = "Work item burndown";
  burndownPendingCount = 1;
  burndownPending = "Pending work items - 2";
  burndownChartCount = 1;
  burndownChartVisible = true;
  burndownEmptyCount = 0;
  burndownEmptyVisible = false;
  tabLabels: string[] = ["Priority work items", "Assignees", "Labels"];
  breakdownPanelText = "";
  breakdownEmptyCount = 0;
  breakdownEmptyVisible = false;
  upsellCount = 1;
  upsellHeading = "On-demand snapshots";
  benefitCount = 6;
  upgradeHref: string | null = "https://airepublic.com/pricing";
  upgradeTarget: string | null = "_blank";
  heroOffsetCount = 1;
  heroOffsetVisible = true;
  heroOffsetText = "UTC -05:00";
  sidebarOffsetCount = 1;
  sidebarOffsetVisible = true;
  sidebarOffsetText = "UTC -05:00";
  tooltipCount = 1;
  tooltipVisible = true;
  tooltipText = "Nov 27, 2026 in your timezone (2026)";
  toastCount = 0;
  addButtons: Array<{ visible: boolean; disabled: boolean }> = [{ visible: true, disabled: false }];
  dialogInputCount = 0;
  dialogAlertCount = 0;
  transferActionCount = 1;
  transferActionVisible = true;
  transferActionText = "Transfer 1 work item";
  hiddenRows: string[] = [];
  transferOptionTexts: string[] = [];
  sidebarDatesTriggerCount = 1;
  sidebarDatesDisabled = false;
  favoriteCount = 1;
  favoriteVisible = true;
  searchToggleCount = 1;
  searchToggleVisible = true;
  searchBoxCount = 0;
  searchInputValue = "";
  filterTriggerCount = 1;
  filterTriggerVisible = true;
  filterPanelText = "Status\nDate range";
  clipboardText = "https://app/cycles/abc";

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path, method, args });
        return this.locator(`${path}.${method}(${render(args)})`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        const call = { path, method, args };
        this.calls.push(call);
        return this.respond(call);
      };
    }
    return node as unknown as Locator;
  }

  respond(call: Call): unknown {
    const { path, method, args } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText") return this.text(path);
    if (method === "inputValue") return this.inputValue(path);
    if (method === "allTextContents") return this.allTexts(path);
    if (method === "allInnerTexts") return path === TRANSFER_NAME_SPANS_PATH ? [...this.transferOptionTexts] : [];
    if (method === "isVisible") return this.visible(path);
    if (method === "isDisabled") return this.disabled(path);
    if (method === "getAttribute") return this.attribute(path, args[0]);
    if (method === "fill") {
      this.fills.push({ path, value: String(args[0]) });
      return undefined;
    }
    if (method === "waitFor" || method === "click" || method === "hover") return undefined;
    throw new Error(`unexpected action ${method} at ${path} with ${JSON.stringify(args)}`);
  }

  private count(path: string): number {
    for (const hidden of this.hiddenRows) {
      // cycleRowRoot already ends with .first(); ensure-row counts it as is.
      if (path === rowFilterPath(hidden) || path === rowPath(hidden)) return 0;
    }
    if (path === HEADING_PATH) return this.headingCount;
    if (path === ACTIVE_NAME_PATH) return this.activeNameCount;
    if (path === CARDS_PATH) return this.heroCardsCount;
    if (path === NO_ACTIVE_PATH) return this.noActiveCount;
    if (path === `${NO_ACTIVE_PATH}.locator("h2").first()`) return this.noActiveCount;
    if (path === `${NO_ACTIVE_PATH}.locator("p").first()`) return this.noActiveCount;
    if (path === CLOSED_PATH) return this.progressClosedCount;
    if (path === GROUP_ROWS_PATH) return this.progressGroupTexts.length;
    if (path === `${PROGRESS_CARD_PATH}.locator("div.text-center h3")`) return this.progressEmptyCount;
    if (path === `${BURNDOWN_CARD_PATH}.locator("h3").first()`) return 1;
    if (path === `${BURNDOWN_CARD_PATH}.getByTestId("hero-burndown-pending")`) return this.burndownPendingCount;
    if (path === `${BURNDOWN_CARD_PATH}.getByTestId("chart-area")`) return this.burndownChartCount;
    if (path === `${BURNDOWN_CARD_PATH}.locator("div.text-center h3")`) return this.burndownEmptyCount;
    if (path === `${BREAKDOWN_CARD_PATH}.getByRole("tab")`) return this.tabLabels.length;
    if (path === `${BREAKDOWN_CARD_PATH}.locator("[role=\\"tabpanel\\"] div.text-center h3")`)
      return this.breakdownEmptyCount;
    if (path === UPSELL_PATH) return this.upsellCount;
    if (path === `${UPSELL_PATH}.locator("h2").first()`) return this.upsellCount;
    if (path === `${BENEFITS_PATH}.locator("li")`) return this.benefitCount;
    if (path === `${UPSELL_PATH}.locator("a").first()`) return this.upgradeHref === null ? 0 : 1;
    if (path === `${HERO_PATH}.getByTestId("cycle-date-offset").first()`) return this.heroOffsetCount;
    if (path === `${SIDEBAR_DATES_PATH}.getByTestId("cycle-date-offset").first()`) return this.sidebarOffsetCount;
    if (path === TOOLTIP_PATH) return this.tooltipCount;
    if (path === TOAST_PATH) return this.toastCount;
    if (path === `${BREAKDOWN_CARD_PATH}.getByRole("tab")`) return this.tabLabels.length;
    if (path === `page.getByRole("button",{"name":"Add cycle"})`) return this.addButtons.length;
    if (path.endsWith(`.locator("input[name=\\"name\\"]")`)) return this.dialogInputCount;
    if (path.endsWith(`.getByRole("alert")`)) return this.dialogAlertCount;
    if (path.includes('getByRole("button",{"name":{}}') && path.includes("cycle-row")) return this.transferActionCount;
    if (path === SEARCH_TOGGLE_PATH) return this.searchToggleCount;
    if (path === SEARCH_BOX_PATH) return this.searchBoxCount;
    if (path === FILTER_TRIGGER_PATH) return this.filterTriggerCount;
    if (path === SIDEBAR_TRIGGER_PATH) return this.sidebarDatesTriggerCount;
    if (path.endsWith(`.getByTestId("cycle-favorite-marker")`)) return this.favoriteCount;
    return 1;
  }

  private text(path: string): string {
    if (path === HEADING_PATH) return this.headingText;
    if (path === ACTIVE_NAME_PATH) return this.activeNameText;
    if (path === `${NO_ACTIVE_PATH}.locator("h2").first()`) return this.emptyTitle;
    if (path === `${NO_ACTIVE_PATH}.locator("p").first()`) return this.emptyDescription;
    if (path === CLOSED_PATH) return this.progressClosedText;
    if (path.startsWith(`${GROUP_ROWS_PATH}.nth(`)) {
      const index = nthIndex(path);
      if (path.endsWith(`.locator("span").first()`))
        return index === null ? "" : (this.progressGroupNames[index] ?? "");
      return index === null ? "" : (this.progressGroupTexts[index] ?? "");
    }
    if (path === `${BURNDOWN_CARD_PATH}.locator("h3").first()`) return this.burndownHeading;
    if (path === `${BURNDOWN_CARD_PATH}.getByTestId("hero-burndown-pending")`) return this.burndownPending;
    if (path.startsWith(`${BREAKDOWN_CARD_PATH}.getByRole("tab").nth(`)) {
      const index = nthIndex(path);
      return index === null ? "" : (this.tabLabels[index] ?? "");
    }
    if (path === `${BREAKDOWN_CARD_PATH}.locator("[role=\\"tabpanel\\"]").first()`) return this.breakdownPanelText;
    if (path === `${UPSELL_PATH}.locator("h2").first()`) return this.upsellHeading;
    if (path === `${HERO_PATH}.getByTestId("cycle-date-offset").first()`) return this.heroOffsetText;
    if (path === `${SIDEBAR_DATES_PATH}.getByTestId("cycle-date-offset").first()`) return this.sidebarOffsetText;
    if (path === TOOLTIP_PATH) return this.tooltipText;
    if (path.includes('getByRole("button",{"name":{}}') && path.includes("cycle-row")) return this.transferActionText;
    if (path === FILTER_PANEL_PATH) return this.filterPanelText;
    return "";
  }

  private inputValue(path: string): string {
    if (path === SEARCH_INPUT_PATH) return this.searchInputValue;
    return "";
  }

  private allTexts(_path: string): string[] {
    return [];
  }

  private visible(path: string): boolean {
    if (path === ACTIVE_NAME_PATH) return this.activeNameVisible;
    if (path.startsWith(`${GROUP_ROWS_PATH}.nth(`)) return true;
    if (path === `${PROGRESS_CARD_PATH}.locator("div.text-center h3").first()`) return this.progressEmptyVisible;
    if (path === `${BURNDOWN_CARD_PATH}.getByTestId("chart-area").first()`) return this.burndownChartVisible;
    if (path === `${BURNDOWN_CARD_PATH}.locator("div.text-center h3").first()`) return this.burndownEmptyVisible;
    if (path.startsWith(`${BREAKDOWN_CARD_PATH}.locator("[data-testid=\\"hero-breakdown-entry\\"]").nth(`)) return true;
    if (path === `${BREAKDOWN_CARD_PATH}.locator("[role=\\"tabpanel\\"] div.text-center h3").first()`)
      return this.breakdownEmptyVisible;
    if (path === `${HERO_PATH}.getByTestId("cycle-date-offset").first()`) return this.heroOffsetVisible;
    if (path === `${SIDEBAR_DATES_PATH}.getByTestId("cycle-date-offset").first()`) return this.sidebarOffsetVisible;
    if (path === TOOLTIP_PATH) return this.tooltipVisible;
    if (path.startsWith(`page.getByRole("button",{"name":"Add cycle"}).nth(`)) {
      const index = nthIndex(path);
      return index === null ? false : (this.addButtons[index]?.visible ?? false);
    }
    if (path.includes('getByRole("button",{"name":{}}') && path.includes("cycle-row"))
      return this.transferActionVisible;
    if (path === `${SEARCH_TOGGLE_PATH}.first()`) return this.searchToggleVisible;
    if (path === `${FILTER_TRIGGER_PATH}.first()`) return this.filterTriggerVisible;
    if (path.endsWith(`.getByTestId("cycle-favorite-marker").first()`)) return this.favoriteVisible;
    if (path === `page.getByTestId("upgrade-pill").first()`) return true;
    return true;
  }

  private disabled(path: string): boolean {
    if (path.startsWith(`page.getByRole("button",{"name":"Add cycle"}).nth(`)) {
      const index = nthIndex(path);
      return index === null ? false : (this.addButtons[index]?.disabled ?? false);
    }
    if (path === SIDEBAR_TRIGGER_PATH) return this.sidebarDatesDisabled;
    return false;
  }

  private attribute(path: string, name: unknown): string | null {
    if (path === `${UPSELL_PATH}.locator("a").first()`) {
      if (name === "href") return this.upgradeHref;
      if (name === "target") return this.upgradeTarget;
    }
    return null;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByTestId", "getByPlaceholder", "locator"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.keyboard = {
      press: async (key: string) => {
        this.keyboardPresses.push(key);
        return undefined;
      },
    };
    node.goto = async (url: string) => {
      this.gotoUrls.push(url);
      return undefined;
    };
    node.waitForLoadState = async () => undefined;
    node.waitForTimeout = async () => undefined;
    node.waitForURL = async (pattern: unknown) => {
      this.waitedUrls.push(pattern);
      return undefined;
    };
    node.route = async (predicate: unknown, handler: (route: Route) => Promise<void>) => {
      this.routes.push({ predicate, handler });
      return undefined;
    };
    node.evaluate = async (fn: unknown) => {
      this.evaluated.push(fn);
      return this.evaluateResult;
    };
    node.context = () => ({
      grantPermissions: async (permissions: string[]) => {
        this.grantedPermissions.push(permissions);
        return undefined;
      },
    });
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }

  callsIncluding(substring: string, method: string): Call[] {
    return this.calls.filter((call) => call.method === method && call.path.includes(substring));
  }
}

describe("cyclesHeroGroupHeading", () => {
  it("reads the disclosure button text", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroGroupHeading()).toBe("Active cycle 1");
    script.headingCount = 0;
    expect(await script.driver().cyclesHeroGroupHeading()).toBe(null);
  });
});

describe("cyclesHeroActiveName", () => {
  it("reads the hero row name and nulls when absent or hidden", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroActiveName()).toBe("Hero cycle");
    script.activeNameVisible = false;
    expect(await script.driver().cyclesHeroActiveName()).toBe(null);
    script.activeNameCount = 0;
    expect(await script.driver().cyclesHeroActiveName()).toBe(null);
  });
});

describe("cyclesHeroEmptyCopy", () => {
  it("reads the no-active title and description, null when cards show", async () => {
    const script = new Recorder();
    script.heroCardsCount = 0;
    script.noActiveCount = 1;
    expect(await script.driver().cyclesHeroEmptyCopy()).toEqual({
      title: "No active cycle",
      description: "No cycle covers today.",
    });
    script.heroCardsCount = 1;
    expect(await script.driver().cyclesHeroEmptyCopy()).toBe(null);
    script.heroCardsCount = 0;
    script.noActiveCount = 0;
    expect(await script.driver().cyclesHeroEmptyCopy()).toBe(null);
  });
});

describe("cyclesHeroProgressGroups", () => {
  it("reads the closed line and the clickable group rows", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroProgressGroups()).toEqual({
      closed: "0/2 Work items closed",
      groups: [
        { name: "Started", text: "Started 1" },
        { name: "Backlog", text: "Backlog 1" },
      ],
    });
    script.progressClosedCount = 0;
    script.progressGroupTexts = [];
    script.progressGroupNames = [];
    expect(await script.driver().cyclesHeroProgressGroups()).toEqual({ closed: null, groups: [] });
  });
});

describe("cyclesHeroProgressEmptyVisible", () => {
  it("reads the empty marker", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroProgressEmptyVisible()).toBe(false);
    script.progressEmptyCount = 1;
    script.progressEmptyVisible = true;
    expect(await script.driver().cyclesHeroProgressEmptyVisible()).toBe(true);
  });
});

describe("cyclesHeroBurndown", () => {
  it("reads heading, pending line, and chart presence", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroBurndown()).toEqual({
      heading: "Work item burndown",
      pending: "Pending work items - 2",
      chart: true,
    });
    script.burndownPendingCount = 0;
    script.burndownChartCount = 0;
    expect(await script.driver().cyclesHeroBurndown()).toEqual({
      heading: "Work item burndown",
      pending: null,
      chart: false,
    });
  });
});

describe("cyclesHeroBurndownEmptyVisible", () => {
  it("reads the empty marker", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroBurndownEmptyVisible()).toBe(false);
    script.burndownEmptyCount = 1;
    script.burndownEmptyVisible = true;
    expect(await script.driver().cyclesHeroBurndownEmptyVisible()).toBe(true);
  });
});

describe("cyclesHeroBreakdownTabs", () => {
  it("reads the tab labels in order, empty when the card is absent", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroBreakdownTabs()).toEqual(["Priority work items", "Assignees", "Labels"]);
    script.tabLabels = [];
    expect(await script.driver().cyclesHeroBreakdownTabs()).toEqual([]);
  });
});

describe("cyclesHeroBreakdownSelectTab", () => {
  it("clicks the tab and waits for its panel", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroBreakdownSelectTab("Assignees");
    expect(script.callsIncluding(`getByRole("tab",{"name":"Assignees"})`, "click")).toHaveLength(1);
    expect(script.callsIncluding(`[data-tab=\\"assignees\\"]`, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHeroBreakdownEntries", () => {
  it("reads the panel text lines", async () => {
    const script = new Recorder();
    script.breakdownPanelText = "S41 started urgent\nUrgent\n";
    expect(await script.driver().cyclesHeroBreakdownEntries()).toEqual(["S41 started urgent", "Urgent"]);
  });
});

describe("cyclesHeroBreakdownEmptyVisible", () => {
  it("reads the panel empty marker", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroBreakdownEmptyVisible()).toBe(false);
    script.breakdownEmptyCount = 1;
    script.breakdownEmptyVisible = true;
    expect(await script.driver().cyclesHeroBreakdownEmptyVisible()).toBe(true);
  });
});

describe("cyclesHeroClickProgressGroup", () => {
  it("clicks the group and waits for the detail address", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroClickProgressGroup("backlog");
    expect(script.callsIncluding(`[data-group=\\"backlog\\"]`, "click")).toHaveLength(1);
    expect(script.waitedUrls).toHaveLength(1);
    const pattern = script.waitedUrls[0] as RegExp;
    expect(pattern.test("/w/projects/p/cycles/cid")).toBe(true);
    expect(pattern.test("/w/projects/p/cycles")).toBe(false);
  });
});

describe("cyclesHeroTapProgressGroup", () => {
  it("clicks without waiting for navigation", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroTapProgressGroup("backlog");
    expect(script.callsIncluding(`[data-group=\\"backlog\\"]`, "click")).toHaveLength(1);
    expect(script.waitedUrls).toHaveLength(0);
  });
});

describe("cyclesHeroClickBreakdownEntry", () => {
  it("clicks the indexed entry and waits for the detail address", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroClickBreakdownEntry(0);
    expect(script.callsIncluding("hero-breakdown-entry", "click")).toHaveLength(1);
    expect(script.waitedUrls).toHaveLength(1);
  });
});

describe("cyclesHeroOpenWorkspaceActives", () => {
  it("opens the actives address and waits for the upsell", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenWorkspaceActives("acme");
    expect(script.gotoUrls).toEqual(["/acme/active-cycles"]);
    expect(script.callsFor(UPSELL_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHeroUpsell", () => {
  it("reads heading, benefit count, and upgrade link", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroUpsell()).toEqual({
      heading: "On-demand snapshots",
      benefits: 6,
      upgradeHref: "https://airepublic.com/pricing",
      upgradeTarget: "_blank",
    });
    script.upgradeHref = null;
    const dropped = await script.driver().cyclesHeroUpsell();
    expect(dropped.upgradeHref).toBe(null);
    expect(dropped.upgradeTarget).toBe(null);
  });
});

describe("cyclesHeroUpsellBadgeVisible", () => {
  it("reads the upgrade pill", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroUpsellBadgeVisible()).toBe(true);
  });
});

describe("cyclesHeroActiveRowOffset", () => {
  it("reads the hero offset chip and nulls when absent", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroActiveRowOffset()).toBe("UTC -05:00");
    script.heroOffsetCount = 0;
    expect(await script.driver().cyclesHeroActiveRowOffset()).toBe(null);
  });
});

describe("cyclesHeroHoverActiveRowDates", () => {
  it("hovers the hero date span", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroHoverActiveRowDates();
    expect(script.callsIncluding("cycle-date-span", "hover")).toHaveLength(1);
  });
});

describe("cyclesHeroSidebarOffset", () => {
  it("reads the sidebar offset chip and nulls when absent", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroSidebarOffset()).toBe("UTC -05:00");
    script.sidebarOffsetCount = 0;
    expect(await script.driver().cyclesHeroSidebarOffset()).toBe(null);
  });
});

describe("cyclesHeroHoverSidebarDates", () => {
  it("hovers the sidebar dates trigger", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroHoverSidebarDates();
    expect(script.callsFor(SIDEBAR_TRIGGER_PATH, "hover")).toHaveLength(1);
  });
});

describe("cyclesHeroTooltipText", () => {
  it("reads the visible tooltip text", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroTooltipText()).toBe("Nov 27, 2026 in your timezone (2026)");
    expect(script.callsFor(TOOLTIP_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHero traffic spy", () => {
  function fakeRoute(method: string, url: string): Route {
    return {
      request: () => ({ method: () => method, url: () => url }),
      continue: async () => undefined,
    } as unknown as Route;
  }

  it("sorts reads and writes into buckets and ignores the rest", async () => {
    const script = new Recorder();
    const driver = script.driver();
    await driver.cyclesHeroBeginTrafficSpy();
    expect(script.routes).toHaveLength(1);
    const handler = (script.routes[0] as ScriptedRoute).handler;
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/p/cycles/"));
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/p/cycles/cid/"));
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/p/cycles/cid/progress/"));
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/p/archived-cycles/"));
    await handler(fakeRoute("POST", "http://x/api/workspaces/w/projects/p/cycles/"));
    await handler(fakeRoute("POST", "http://x/api/workspaces/w/projects/p/cycles/date-check/"));
    await handler(fakeRoute("PATCH", "http://x/api/workspaces/w/projects/p/cycles/cid/"));
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/"));
    expect(await driver.cyclesHeroTrafficCounts()).toEqual({ cycleReads: 3, archivedReads: 1, writes: 3 });
  });

  it("resets the counters when rearmed", async () => {
    const script = new Recorder();
    const driver = script.driver();
    await driver.cyclesHeroBeginTrafficSpy();
    const handler = (script.routes[0] as ScriptedRoute).handler;
    await handler(fakeRoute("GET", "http://x/api/workspaces/w/projects/p/cycles/"));
    await driver.cyclesHeroBeginTrafficSpy();
    expect(await driver.cyclesHeroTrafficCounts()).toEqual({ cycleReads: 0, archivedReads: 0, writes: 0 });
  });
});

describe("cyclesHeroDelaySidebarReads", () => {
  function fakeRoute(method: string, onContinue: () => void): Route {
    return {
      request: () => ({ method: () => method, url: () => "http://x/api/workspaces/w/projects/p/cycles/c/progress/" }),
      continue: async () => {
        onContinue();
      },
    } as unknown as Route;
  }

  it("holds only the progress and analytics reads, never the loader's detail read", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroDelaySidebarReads(1);
    expect(script.routes).toHaveLength(1);
    const predicate = (script.routes[0] as ScriptedRoute).predicate as (url: URL) => boolean;
    const match = (path: string) => predicate(new URL(`http://x${path}`));
    expect(match("/api/workspaces/w/projects/p/cycles/c/progress/")).toBe(true);
    expect(match("/api/workspaces/w/projects/p/cycles/c/analytics/")).toBe(true);
    expect(match("/api/workspaces/w/projects/p/cycles/c/analytics/?kind=issues")).toBe(true);
    expect(match("/api/workspaces/w/projects/p/cycles/c/")).toBe(false);
    expect(match("/api/workspaces/w/projects/p/cycles/")).toBe(false);
    expect(match("/api/workspaces/w/projects/p/cycles/c/cycle-issues/")).toBe(false);
    expect(match("/api/workspaces/w/projects/p/cycles/c/user-properties/")).toBe(false);
    let continued = 0;
    const handler = (script.routes[0] as ScriptedRoute).handler;
    await handler(
      fakeRoute("GET", () => {
        continued += 1;
      })
    );
    await handler(
      fakeRoute("POST", () => {
        continued += 1;
      })
    );
    expect(continued).toBe(2);
  });
});

describe("cyclesHeroFocusWindow", () => {
  it("dispatches focus and visibility events", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroFocusWindow();
    expect(script.evaluated).toHaveLength(1);
  });
});

describe("cyclesHero create dialog", () => {
  it("opens via the header button", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenCreate();
    expect(script.callsIncluding(`getByRole("button",{"name":"Add cycle"})`, "click")).toHaveLength(1);
  });

  it("fills the title", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroFillCreateName("Fresh cycle");
    expect(script.fills).toHaveLength(1);
    expect(script.fills[0]?.value).toBe("Fresh cycle");
  });

  it("submits through the shared stale-toast guard", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroSubmitCreate();
    expect(script.callsIncluding(`getByRole("button",{"name":"Create cycle"})`, "click")).toHaveLength(1);
  });

  it("reads the dialog open state", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroCreateOpen()).toBe(false);
    script.dialogInputCount = 1;
    expect(await script.driver().cyclesHeroCreateOpen()).toBe(true);
  });
});

describe("cyclesHero transfer action", () => {
  it("reads the row banner text and nulls for guests", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroTransferBanner("Done")).toBe("Transfer 1 work item");
    script.transferActionCount = 0;
    expect(await script.driver().cyclesHeroTransferBanner("Done")).toBe(null);
  });

  it("opens the transfer dialog from the row", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenTransfer("Done");
    expect(script.callsIncluding(rowPath("Done"), "hover")).toHaveLength(1);
    // Regex accessible names serialize to {"name":{}} in the recording.
    expect(script.callsIncluding(`getByRole("button",{"name":{}})`, "click")).toHaveLength(1);
    expect(script.callsIncluding("Search for a cycle", "waitFor")).toHaveLength(1);
  });

  it("expands the completed group for a hidden row", async () => {
    const script = new Recorder();
    script.hiddenRows = ["Done"];
    expect(await script.driver().cyclesHeroTransferBanner("Done")).toBe("Transfer 1 work item");
    expect(script.callsIncluding("cycle-group-completed", "click")).toHaveLength(1);
  });

  it("searches, lists, and picks transfer targets", async () => {
    const script = new Recorder();
    // The driver reads the name spans, so the status pill never leaks in.
    script.transferOptionTexts = ["Target", "Other"];
    await script.driver().cyclesHeroTransferSearch("Targ");
    expect(script.fills).toHaveLength(1);
    expect(await script.driver().cyclesHeroTransferOptions()).toEqual(["Target", "Other"]);
    expect(script.callsFor(TRANSFER_NAME_SPANS_PATH, "allInnerTexts")).toHaveLength(1);
    await script.driver().cyclesHeroTransferPick("Target");
    expect(script.callsIncluding(`getByRole("option",{"name":{}})`, "click")).toHaveLength(1);
  });
});

describe("cyclesHeroCreateButtonState", () => {
  it("reports absent, disabled, and enabled", async () => {
    const script = new Recorder();
    script.addButtons = [];
    expect(await script.driver().cyclesHeroCreateButtonState()).toBe("absent");
    script.addButtons = [{ visible: true, disabled: true }];
    expect(await script.driver().cyclesHeroCreateButtonState()).toBe("disabled");
    script.addButtons = [
      { visible: false, disabled: false },
      { visible: true, disabled: false },
    ];
    expect(await script.driver().cyclesHeroCreateButtonState()).toBe("enabled");
  });
});

describe("cyclesHeroCloseMenus", () => {
  it("presses Escape", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroCloseMenus();
    expect(script.keyboardPresses).toEqual(["Escape"]);
  });
});

describe("cyclesHeroSidebarDateDisabled", () => {
  it("reads the trigger state and treats absence as locked", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroSidebarDateDisabled()).toBe(false);
    script.sidebarDatesDisabled = true;
    expect(await script.driver().cyclesHeroSidebarDateDisabled()).toBe(true);
    script.sidebarDatesTriggerCount = 0;
    expect(await script.driver().cyclesHeroSidebarDateDisabled()).toBe(true);
  });
});

describe("cyclesHeroFavoriteVisible", () => {
  it("reads the row star", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroFavoriteVisible("Cycle")).toBe(true);
    script.favoriteCount = 0;
    expect(await script.driver().cyclesHeroFavoriteVisible("Cycle")).toBe(false);
  });
});

describe("cyclesHero search and filter", () => {
  it("reports search visibility from the toggle or the box", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroSearchVisible()).toBe(true);
    script.searchToggleCount = 0;
    script.searchToggleVisible = false;
    script.searchBoxCount = 1;
    expect(await script.driver().cyclesHeroSearchVisible()).toBe(true);
    script.searchBoxCount = 0;
    expect(await script.driver().cyclesHeroSearchVisible()).toBe(false);
  });

  it("opens collapsed search and skips the toggle when expanded", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroSearchOpen();
    expect(script.callsFor(SEARCH_TOGGLE_PATH, "click")).toHaveLength(1);
    const expanded = new Recorder();
    expanded.searchBoxCount = 1;
    await expanded.driver().cyclesHeroSearchOpen();
    expect(expanded.callsFor(SEARCH_TOGGLE_PATH, "click")).toHaveLength(0);
  });

  it("types and reads the query, empty while collapsed", async () => {
    const script = new Recorder();
    script.searchBoxCount = 1;
    script.searchInputValue = "zzz";
    await script.driver().cyclesHeroSearchType("zzz");
    expect(script.fills).toHaveLength(1);
    expect(await script.driver().cyclesHeroSearchText()).toBe("zzz");
    script.searchBoxCount = 0;
    expect(await script.driver().cyclesHeroSearchText()).toBe("");
  });

  it("reports filter trigger visibility", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroFilterVisible()).toBe(true);
    script.filterTriggerCount = 0;
    script.filterTriggerVisible = false;
    expect(await script.driver().cyclesHeroFilterVisible()).toBe(false);
  });

  it("opens the filter menu and reads its text lines", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroFilterMenuTexts()).toEqual(["Status", "Date range"]);
    expect(script.callsFor(FILTER_TRIGGER_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesHeroOpenPeek", () => {
  it("hovers the row, clicks the eye, and waits for the panel", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenPeek("Upcoming");
    expect(script.callsIncluding(rowPath("Upcoming"), "hover")).toHaveLength(1);
    expect(script.callsIncluding("Quick look at", "click")).toHaveLength(1);
    expect(script.callsIncluding("cycle-preview", "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHeroOpenPeekRaw", () => {
  it("addresses the list with the peekCycle key", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenPeekRaw("acme", "pid", "cid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles?peekCycle=cid"]);
  });
});

describe("cyclesHeroRowMenuClick", () => {
  it("clicks the menu entry by heading", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroRowMenuClick("Copy link");
    const scoped = script.calls.some(
      (call) => call.method === "locator" && JSON.stringify(call.args).includes("Copy link")
    );
    expect(scoped).toBe(true);
    expect(script.callsIncluding(`getByRole("menuitem")`, "click")).toHaveLength(1);
  });
});

describe("cyclesHero clipboard", () => {
  it("grants permissions and reads the clipboard text", async () => {
    const script = new Recorder();
    script.evaluateResult = "https://app/cycles/abc";
    await script.driver().cyclesHeroGrantClipboard();
    expect(script.grantedPermissions).toEqual([["clipboard-read", "clipboard-write"]]);
    expect(await script.driver().cyclesHeroReadClipboard()).toBe("https://app/cycles/abc");
  });
});
