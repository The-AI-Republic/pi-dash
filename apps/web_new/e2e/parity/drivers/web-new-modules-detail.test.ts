// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the module detail driver methods (NEWFRONT-802,
// MOD-029/030/032): opening, the tab title, the badge and its tooltip,
// the trail, the switcher, the unknown-module state, layout switching,
// the filter toggle, the display group entries, the small-screen bar,
// the analytics entry and dialog, and the sidebar toggle with its
// persisted collapse flag. The area gate (NEWFRONT-808) adds the MOD-033
// sidebar field reads and writes, the counts, and the slow-fetch loader.
// A fake Page answers from a path-keyed table so each case pins the hook a
// method reads or the control it activates; no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  texts?: string[];
  text?: string;
  value?: string;
  attrs?: Record<string, string | null>;
  disabled?: boolean;
  evaluate?: unknown;
  evaluated?: unknown;
}

type Table = Record<string, Answer>;

const ROOT = "page";

function keyFor(parent: string, kind: string, args: unknown[]): string {
  const rendered = args
    .map((arg) => (arg && typeof arg === "object" ? String((arg as { name?: string }).name ?? "") : String(arg)))
    .join(",");
  return `${parent} ${kind}(${rendered})`;
}

/**
 * A Playwright-shaped fake. Locators are addressed by the chain of calls
 * that built them, so a table key such as
 * `page testid(module-detail-crumbs) testid(module-crumb-project)` answers
 * `page.getByTestId("module-detail-crumbs").getByTestId("module-crumb-project")`.
 */
class FakePage {
  readonly clicks: string[] = [];
  readonly keys: string[] = [];
  readonly fills: string[] = [];
  readonly presses: string[] = [];
  readonly hovers: string[] = [];
  readonly pageCalls: string[] = [];
  readonly routes: Array<{
    matcher: unknown;
    handler: (route: {
      request: () => { method: () => string };
      fulfill: (answer: unknown) => Promise<void>;
      continue: () => Promise<void>;
    }) => Promise<void>;
  }> = [];
  readonly fulfilled: unknown[] = [];
  readonly continued: string[] = [];
  path = "/acme/projects/p1/modules/m1";
  titleValue = "Website - Delivery";
  closed = false;

  constructor(private readonly table: Table = {}) {}

  private answer(id: string): Answer & { visible: boolean; count: number } {
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.startsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    return { visible: true, count: 1, ...(candidates[0]?.[1] ?? {}) };
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: () => self.locator(id),
      filter: (options: { has?: Locator; hasText?: string | RegExp }) =>
        self.locator(`${id} has(${String((options.has as never)?.["__id"] ?? "")},${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string }) =>
        self.locator(`${id} role(${role},${options?.name ?? ""})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      fill: async (value: string) => {
        self.fills.push(`${id}=${value}`);
      },
      press: async (key: string) => {
        self.presses.push(`${id}:${key}`);
      },
      hover: async () => {
        self.hovers.push(id);
      },
      waitFor: async () => undefined,
      evaluate: async () => self.answer(id).evaluate ?? self.answer(id).evaluated ?? {},
      evaluateAll: async () => self.answer(id).evaluated ?? [],
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => {
        self.keys.push(`testid:${testId}`);
        return self.locator(keyFor(ROOT, "testid", [testId]));
      },
      getByRole: (role: string, options?: { name?: string }) => {
        self.keys.push(`role:${role}:${options?.name ?? ""}`);
        return self.locator(keyFor(ROOT, "role", [role, options?.name ?? ""]));
      },
      getByPlaceholder: (placeholder: string) => {
        self.keys.push(`placeholder:${placeholder}`);
        return self.locator(keyFor(ROOT, "placeholder", [placeholder]));
      },
      locator: (selector: string) => {
        self.keys.push(`css:${selector}`);
        return self.locator(keyFor(ROOT, "css", [selector]));
      },
      url: () => `https://app.test${self.path}`,
      title: async () => self.titleValue,
      evaluate: async (callback: (arg?: unknown) => unknown) => callback(),
      goto: async (url: string) => {
        self.pageCalls.push(`goto:${url}`);
      },
      reload: async () => {
        self.pageCalls.push("reload");
      },
      waitForLoadState: async () => {
        self.pageCalls.push("waitForLoadState");
      },
      waitForURL: async (pattern: RegExp) => {
        self.pageCalls.push(`waitForURL:${String(pattern)}`);
      },
      waitForTimeout: async () => undefined,
      isClosed: () => self.closed,
      keyboard: { press: async () => undefined },
      route: async (matcher: unknown, handler: (route: unknown) => Promise<void>) => {
        self.routes.push({ matcher, handler: handler as never });
      },
      unrouteAll: async () => {
        self.pageCalls.push("unrouteAll");
      },
      context: () => ({ addCookies: async () => undefined }),
    };
    return page as unknown as Page;
  }
}

function harness(table: Table = {}): { fake: FakePage; driver: WebNewDriver } {
  const fake = new FakePage(table);
  return { fake, driver: new WebNewDriver(fake.asPage()) };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("modulesOpenDetail", () => {
  it("opens the detail address and settles on the trail", async () => {
    const { fake, driver } = harness();
    await driver.modulesOpenDetail("acme", "p1", "m1", []);
    expect(fake.pageCalls).toContain("goto:/acme/projects/p1/modules/m1");
    expect(fake.keys).toContain("testid:module-detail-crumbs");
  });
});

describe("modulesDetailPageTitle", () => {
  it("reads the document title", async () => {
    const { driver } = harness();
    expect(await driver.modulesDetailPageTitle()).toBe("Website - Delivery");
  });
});

describe("modulesDetailBadgeText", () => {
  const badge = "page testid(module-detail-badge)";

  it("reads the badge count", async () => {
    const { driver } = harness({ [badge]: { text: " 2 " } });
    expect(await driver.modulesDetailBadgeText()).toBe("2");
  });

  it("reads null without a badge", async () => {
    const { driver } = harness({ [badge]: { count: 0 } });
    expect(await driver.modulesDetailBadgeText()).toBe(null);
  });

  it("reads null for a hidden or blank badge", async () => {
    const { driver: hidden } = harness({ [badge]: { visible: false, text: "2" } });
    expect(await hidden.modulesDetailBadgeText()).toBe(null);
    const { driver: blank } = harness({ [badge]: { text: "  " } });
    expect(await blank.modulesDetailBadgeText()).toBe(null);
  });
});

describe("modulesDetailBadgeTooltip", () => {
  const badge = "page testid(module-detail-badge)";

  it("hovers the badge and reads the tooltip", async () => {
    const { fake, driver } = harness({
      [badge]: { count: 1 },
      "page role(tooltip,)": { text: "There are 2 work items in this module" },
    });
    expect(await driver.modulesDetailBadgeTooltip()).toBe("There are 2 work items in this module");
    expect(fake.hovers).toEqual([badge]);
  });

  it("reads null without a badge", async () => {
    const { driver } = harness({ [badge]: { count: 0 } });
    expect(await driver.modulesDetailBadgeTooltip()).toBe(null);
  });
});

describe("modulesDetailCrumbs", () => {
  const trail = "page testid(module-detail-crumbs)";

  it("splits the trail on the fixed middle crumb", async () => {
    const { driver } = harness({ [trail]: { text: "Website   Modules   Delivery" } });
    expect(await driver.modulesDetailCrumbs()).toEqual(["Website", "Modules", "Delivery"]);
  });

  it("reads a single crumb without the middle marker", async () => {
    const { driver } = harness({ [trail]: { text: "Delivery" } });
    expect(await driver.modulesDetailCrumbs()).toEqual(["Delivery"]);
  });

  it("reads no crumbs from an empty trail", async () => {
    const { driver } = harness({ [trail]: { text: "  " } });
    expect(await driver.modulesDetailCrumbs()).toEqual([]);
  });
});

describe("modulesDetailClickModulesCrumb", () => {
  it("activates the Modules crumb", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailClickModulesCrumb();
    expect(fake.clicks).toEqual(["page testid(module-crumb-modules)"]);
  });
});

describe("modulesDetailClickProjectCrumb", () => {
  it("activates the project crumb", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailClickProjectCrumb("Website");
    expect(fake.clicks).toEqual(["page testid(module-crumb-project)"]);
  });
});

describe("modulesDetailSwitcherOpen", () => {
  it("opens the switcher and waits for its options", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailSwitcherOpen();
    expect(fake.clicks).toEqual(["page testid(module-switcher-trigger)"]);
    expect(fake.keys).toContain("role:option:");
  });
});

describe("modulesDetailSwitcherOptions", () => {
  it("trims the option labels and drops blanks", async () => {
    const { driver } = harness({ "page role(option,)": { texts: [" Alpha ", "", "Beta "] } });
    expect(await driver.modulesDetailSwitcherOptions()).toEqual(["Alpha", "Beta"]);
  });
});

describe("modulesDetailSwitcherSearch", () => {
  it("fills the module search box", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailSwitcherSearch("Beta");
    expect(fake.fills).toEqual(["page role(combobox,Search modules)=Beta"]);
  });
});

describe("modulesDetailSwitcherPick", () => {
  it("picks the named option", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailSwitcherPick("Beta");
    expect(fake.clicks).toEqual(["page role(option,Beta)"]);
  });
});

describe("modulesDetailUnknownTitle", () => {
  const scope = "page testid(modules-gone-away)";

  it("reads the gone-away title span", async () => {
    const { driver } = harness({ [`${scope} css(h3 span)`]: { text: "Module does not exist" } });
    expect(await driver.modulesDetailUnknownTitle()).toBe("Module does not exist");
  });

  it("reads null off the gone-away view", async () => {
    const { driver } = harness({ [scope]: { count: 0 } });
    expect(await driver.modulesDetailUnknownTitle()).toBe(null);
  });

  it("reads null for a blank title", async () => {
    const { driver } = harness({ [`${scope} css(h3 span)`]: { text: "  " } });
    expect(await driver.modulesDetailUnknownTitle()).toBe(null);
  });
});

describe("modulesDetailUnknownBack", () => {
  it("activates the back-to-list entry", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailUnknownBack();
    expect(fake.clicks).toEqual(["page testid(modules-gone-away) role(button,Back to modules)"]);
  });
});

describe("modulesDetailSwitchLayout", () => {
  it("delegates to the shared layout switch", async () => {
    const { driver } = harness();
    const shared = vi.spyOn(driver, "layoutsSwitchTo").mockResolvedValue(undefined);
    await driver.modulesDetailSwitchLayout("kanban");
    expect(shared).toHaveBeenCalledWith("kanban");
  });
});

describe("modulesDetailFilterToggleVisible", () => {
  const header = "page testid(issues-header)";

  it("sees the toggle with conditions", async () => {
    const { driver } = harness({ [`${header} testid(rich-filter-toggle)`]: { visible: true } });
    expect(await driver.modulesDetailFilterToggleVisible()).toBe(true);
  });

  it("sees the add entry without conditions", async () => {
    const { driver } = harness({
      [`${header} testid(rich-filter-toggle)`]: { count: 0 },
      [`${header} testid(rich-filter-add)`]: { visible: true },
    });
    expect(await driver.modulesDetailFilterToggleVisible()).toBe(true);
  });

  it("misses both entries when no filter control renders", async () => {
    const { driver } = harness({
      [`${header} testid(rich-filter-toggle)`]: { count: 0 },
      [`${header} testid(rich-filter-add)`]: { count: 0 },
    });
    expect(await driver.modulesDetailFilterToggleVisible()).toBe(false);
  });
});

describe("modulesDetailDisplayGroupedOptions", () => {
  const section = "page testid(display-section-group)";

  it("reads the group entries with their disabled states", async () => {
    const { driver } = harness({ [`${section} role(button,)`]: { count: 2, text: "Cycle", disabled: true } });
    expect(await driver.modulesDetailDisplayGroupedOptions()).toEqual([
      { label: "Cycle", disabled: true },
      { label: "Cycle", disabled: true },
    ]);
  });

  it("drops blank entries", async () => {
    const { driver } = harness({ [`${section} role(button,)`]: { count: 1, text: "  " } });
    expect(await driver.modulesDetailDisplayGroupedOptions()).toEqual([]);
  });

  it("reads no entries from an empty section", async () => {
    const { driver } = harness({ [`${section} role(button,)`]: { count: 0 } });
    expect(await driver.modulesDetailDisplayGroupedOptions()).toEqual([]);
  });
});

describe("modulesDetailMobileOfferedLayouts", () => {
  it("maps the menu labels to layout keys in order", async () => {
    const { fake, driver } = harness({ "page role(menuitem,)": { texts: ["List", "Board", "Calendar"] } });
    expect(await driver.modulesDetailMobileOfferedLayouts()).toEqual(["list", "kanban", "calendar"]);
    expect(fake.clicks).toEqual(["page testid(mobile-layout-trigger)"]);
  });

  it("drops labels outside the layout vocabulary", async () => {
    const { driver } = harness({ "page role(menuitem,)": { texts: ["List", "Bogus"] } });
    expect(await driver.modulesDetailMobileOfferedLayouts()).toEqual(["list"]);
  });
});

describe("modulesDetailMobileSwitchTo", () => {
  it("picks the layout's menu entry", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailMobileSwitchTo("calendar");
    expect(fake.clicks).toEqual(["page testid(mobile-layout-trigger)", "page role(menuitem,Calendar)"]);
  });
});

describe("modulesDetailMobileDisplayVisible", () => {
  const trigger = "page testid(issues-mobile-header) testid(display-options-trigger)";

  it("sees the bar's display entry", async () => {
    const { driver } = harness({ [trigger]: { visible: true } });
    expect(await driver.modulesDetailMobileDisplayVisible()).toBe(true);
  });

  it("misses the display entry when it renders nowhere", async () => {
    const { driver } = harness({ [trigger]: { count: 0 } });
    expect(await driver.modulesDetailMobileDisplayVisible()).toBe(false);
  });
});

describe("modulesDetailMobileFilterToggleVisible", () => {
  const bar = "page testid(issues-mobile-header)";

  it("misses the guarded toggle", async () => {
    const { driver } = harness({
      [`${bar} testid(rich-filter-toggle)`]: { visible: false },
      [`${bar} testid(rich-filter-add)`]: { count: 0 },
    });
    expect(await driver.modulesDetailMobileFilterToggleVisible()).toBe(false);
  });

  it("sees an unguarded toggle", async () => {
    const { driver } = harness({ [`${bar} testid(rich-filter-toggle)`]: { visible: true } });
    expect(await driver.modulesDetailMobileFilterToggleVisible()).toBe(true);
  });
});

describe("modulesDetailHeaderAddVisible", () => {
  const button = "page testid(issues-header) testid(module-add-work-item)";

  it("sees the desktop header add entry", async () => {
    const { driver } = harness({ [button]: { visible: true } });
    expect(await driver.modulesDetailHeaderAddVisible()).toBe(true);
  });

  it("misses the entry for a guest", async () => {
    const { driver } = harness({ [button]: { count: 0 } });
    expect(await driver.modulesDetailHeaderAddVisible()).toBe(false);
  });
});

describe("modulesDetailHeaderAddClick", () => {
  it("clicks the desktop header add entry", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailHeaderAddClick();
    expect(fake.clicks).toEqual(["page testid(issues-header) testid(module-add-work-item)"]);
  });
});

describe("modulesDetailFailNextIssuesAdd", () => {
  function answer(fake: FakePage, method: string) {
    const registered = fake.routes[0];
    if (registered === undefined) throw new Error("no route registered");
    return registered.handler({
      request: () => ({ method: () => method }),
      fulfill: async (body) => {
        fake.fulfilled.push(body);
      },
      continue: async () => {
        fake.continued.push(method);
      },
    });
  }

  it("refuses the module-issues POST and passes reads through", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailFailNextIssuesAdd();
    expect(fake.routes).toHaveLength(1);
    expect(fake.routes[0]?.matcher).toBe("**/modules/*/issues/");
    await answer(fake, "POST");
    await answer(fake, "GET");
    expect(fake.fulfilled).toEqual([{ status: 500, contentType: "application/json", body: "{}" }]);
    expect(fake.continued).toEqual(["GET"]);
  });
});

describe("modulesDetailPickerSearch", () => {
  it("fills the picker's search box", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailPickerSearch("plain");
    expect(fake.fills).toEqual(["page placeholder(Type to search)=plain"]);
  });
});

describe("modulesDetailAnalyticsVisible", () => {
  const entry = "page testid(issues-header) testid(module-analytics-entry)";

  it("sees the desktop analytics entry", async () => {
    const { driver } = harness({ [entry]: { visible: true } });
    expect(await driver.modulesDetailAnalyticsVisible()).toBe(true);
  });

  it("misses the entry for a guest", async () => {
    const { driver } = harness({ [entry]: { count: 0 } });
    expect(await driver.modulesDetailAnalyticsVisible()).toBe(false);
  });
});

describe("modulesDetailOpenAnalytics", () => {
  it("opens the dialog from the desktop entry", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailOpenAnalytics();
    expect(fake.clicks).toEqual(["page testid(issues-header) testid(module-analytics-entry)"]);
    expect(fake.keys).toContain("testid:issues-analytics-body");
  });
});

describe("modulesDetailAnalyticsDialogVisible", () => {
  it("reads the dialog body visibility", async () => {
    const { driver: open } = harness({ "page testid(issues-analytics-body)": { visible: true } });
    expect(await open.modulesDetailAnalyticsDialogVisible()).toBe(true);
    const { driver: closed } = harness({ "page testid(issues-analytics-body)": { count: 0 } });
    expect(await closed.modulesDetailAnalyticsDialogVisible()).toBe(false);
  });
});

describe("modulesDetailDismissAnalytics", () => {
  it("dismisses without reading the page", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailDismissAnalytics();
    expect(fake.keys).toEqual(["testid:issues-analytics-body"]);
  });
});

describe("modulesDetailToggleSidebar", () => {
  it("activates the sidebar toggle", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailToggleSidebar();
    expect(fake.clicks).toEqual(["page testid(module-detail-sidebar-toggle)"]);
  });
});

describe("modulesDetailSidebarVisible", () => {
  it("reads the sidebar panel visibility", async () => {
    const { driver: shown } = harness({ "page testid(modules-analytics-sidebar)": { visible: true } });
    expect(await shown.modulesDetailSidebarVisible()).toBe(true);
    const { driver: hidden } = harness({ "page testid(modules-analytics-sidebar)": { count: 0 } });
    expect(await hidden.modulesDetailSidebarVisible()).toBe(false);
  });
});

describe("modulesDetailSidebarCollapsedStored", () => {
  it("reads the stored collapse flag", async () => {
    const fake = new FakePage();
    const page = fake.asPage();
    vi.spyOn(page, "evaluate").mockResolvedValue(true);
    expect(await new WebNewDriver(page).modulesDetailSidebarCollapsedStored()).toBe(true);
  });

  it("reads expanded without stored state", async () => {
    const { driver } = harness();
    expect(await driver.modulesDetailSidebarCollapsedStored()).toBe(false);
  });
});

describe("modulesDetailCloseSidebarViaPanel", () => {
  it("activates the panel's own close control", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailCloseSidebarViaPanel();
    expect(fake.clicks).toEqual(["page testid(modules-sidebar-close)"]);
  });
});

describe("modulesDetailMobileOpenAnalytics", () => {
  it("opens the dialog from the narrow bar entry", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailMobileOpenAnalytics();
    expect(fake.clicks).toEqual(["page testid(issues-mobile-header) testid(module-analytics-entry)"]);
    expect(fake.keys).toContain("testid:issues-analytics-body");
  });
});

// Sidebar field reads and writes (NEWFRONT-808 gate, MOD-033): status,
// identity, dates, lead, members, counts, points gating, and the slow
// fetch loader. Keys compose the sidebar scope with the row or control
// each method addresses.
const SIDEBAR = "page testid(modules-analytics-sidebar)";

function pillKey(label: string): string {
  return `${SIDEBAR} role(button,${label})`;
}

describe("modulesDetailSidebarName", () => {
  it("reads the sidebar name input", async () => {
    const { driver } = harness({ [`${SIDEBAR} testid(modules-sidebar-name)`]: { value: " Delivery " } });
    expect(await driver.modulesDetailSidebarName()).toBe("Delivery");
  });

  it("reads null when the name is blank or absent", async () => {
    const blank = harness({ [`${SIDEBAR} testid(modules-sidebar-name)`]: { value: "  " } });
    expect(await blank.driver.modulesDetailSidebarName()).toBe(null);
    const absent = harness({ [`${SIDEBAR} testid(modules-sidebar-name)`]: { count: 0 } });
    expect(await absent.driver.modulesDetailSidebarName()).toBe(null);
  });
});

describe("modulesDetailSidebarStatus", () => {
  const OTHERS = ["Backlog", "Planned", "Paused", "Completed", "Cancelled"];

  it("returns the visible pill label", async () => {
    const table: Table = { [pillKey("In Progress")]: { count: 1, visible: true } };
    for (const label of OTHERS) table[pillKey(label)] = { count: 0 };
    const { driver } = harness(table);
    expect(await driver.modulesDetailSidebarStatus()).toBe("In Progress");
  });

  it("returns null when no pill renders", async () => {
    const table: Table = { [pillKey("In Progress")]: { count: 0 } };
    for (const label of OTHERS) table[pillKey(label)] = { count: 0 };
    const { driver } = harness(table);
    expect(await driver.modulesDetailSidebarStatus()).toBe(null);
  });
});

describe("modulesDetailSidebarSetStatus", () => {
  it("throws when the pill never rendered", async () => {
    const table: Table = {};
    for (const label of ["Backlog", "Planned", "In Progress", "Paused", "Completed", "Cancelled"]) {
      table[pillKey(label)] = { count: 0 };
    }
    const { driver } = harness(table);
    await expect(driver.modulesDetailSidebarSetStatus("In Progress")).rejects.toThrow(/never rendered/);
  });

  it("activates the trigger and option, then verifies the pill", async () => {
    const table: Table = { [pillKey("In Progress")]: { count: 1, visible: true } };
    for (const label of ["Backlog", "Planned", "Paused", "Completed", "Cancelled"]) {
      table[pillKey(label)] = { count: 0 };
    }
    const { fake, driver } = harness(table);
    await driver.modulesDetailSidebarSetStatus("In Progress");
    expect(fake.clicks).toContain(pillKey("In Progress"));
    expect(fake.clicks).toContain("page role(option,In Progress)");
  });
});

describe("modulesDetailSidebarDescription", () => {
  it("reads the description field", async () => {
    const { driver } = harness({ [`${SIDEBAR} testid(modules-sidebar-description)`]: { value: "Why" } });
    expect(await driver.modulesDetailSidebarDescription()).toBe("Why");
  });

  it("reads null when the field is absent", async () => {
    const { driver } = harness({ [`${SIDEBAR} testid(modules-sidebar-description)`]: { count: 0 } });
    expect(await driver.modulesDetailSidebarDescription()).toBe(null);
  });
});

describe("modulesDetailSidebarDateText", () => {
  it("reads the trigger label whitespace-collapsed", async () => {
    const { driver } = harness({
      [`${SIDEBAR} testid(modules-sidebar-dates-trigger)`]: { text: "Sep 01,  2026   Oct 18, 2026" },
    });
    expect(await driver.modulesDetailSidebarDateText()).toBe("Sep 01, 2026 Oct 18, 2026");
  });
});

describe("modulesDetailSidebarDateDisabled", () => {
  it("reads the trigger disabled flag", async () => {
    const key = `${SIDEBAR} testid(modules-sidebar-dates-trigger)`;
    expect(await harness({ [key]: { disabled: true } }).driver.modulesDetailSidebarDateDisabled()).toBe(true);
    expect(await harness({ [key]: { disabled: false } }).driver.modulesDetailSidebarDateDisabled()).toBe(false);
  });
});

describe("modulesDetailSidebarOpenDatePicker", () => {
  const CALENDAR = "page testid(modules-sidebar-dates-calendar)";
  const TRIGGER = `${SIDEBAR} testid(modules-sidebar-dates-trigger)`;

  it("clicks the trigger when the calendar is hidden", async () => {
    const { fake, driver } = harness({ [CALENDAR]: { visible: false } });
    await driver.modulesDetailSidebarOpenDatePicker();
    expect(fake.clicks).toEqual([TRIGGER]);
  });

  it("leaves an open calendar alone", async () => {
    const { fake, driver } = harness({ [CALENDAR]: { visible: true } });
    await driver.modulesDetailSidebarOpenDatePicker();
    expect(fake.clicks).toEqual([]);
  });
});

describe("modulesDetailPickDateDay", () => {
  const CALENDAR = "page testid(modules-sidebar-dates-calendar)";

  it("strips ordinals and clicks the visible day", async () => {
    const { fake, driver } = harness({ [`${CALENDAR} role(button,October 15, 2026)`]: { count: 1, visible: true } });
    await driver.modulesDetailPickDateDay("October 15th, 2026");
    expect(fake.clicks).toEqual([`${CALENDAR} role(button,October 15, 2026)`]);
  });

  it("walks forward a bounded number of months, then throws", async () => {
    const { fake, driver } = harness({ [`${CALENDAR} role(button,October 15, 2026)`]: { count: 0 } });
    await expect(driver.modulesDetailPickDateDay("October 15th, 2026")).rejects.toThrow(/never showed/);
    expect(fake.clicks).toHaveLength(24);
    expect(fake.clicks[0]).toBe(`${CALENDAR} role(button,Next month)`);
  });
});

describe("modulesDetailDismissDatePicker", () => {
  it("resolves once the calendar detaches", async () => {
    const { driver } = harness();
    await expect(driver.modulesDetailDismissDatePicker()).resolves.toBe(undefined);
  });
});

describe("modulesDetailSidebarLead", () => {
  const ROW = `${SIDEBAR} role(group,Lead)`;

  it("returns the initial plus the resolved name when set", async () => {
    const { driver } = harness({
      [`${ROW} css(span[role="img"])`]: { count: 1, attrs: { "aria-label": "Ada Lovelace" } },
    });
    expect(await driver.modulesDetailSidebarLead()).toBe("A Ada Lovelace");
  });

  it("returns the prompt when unset", async () => {
    const { driver } = harness({
      [`${ROW} css(span[role="img"])`]: { count: 0 },
      [`${ROW} role(button,)`]: { count: 1, text: "Lead" },
    });
    expect(await driver.modulesDetailSidebarLead()).toBe("Lead");
  });

  it("falls back to the trigger name text when the avatar label is absent", async () => {
    const { driver } = harness({
      [`${ROW} css(span[role="img"])`]: { count: 1, attrs: {} },
      [`${ROW} css(button span.truncate)`]: { count: 1, text: "Ada Lovelace" },
    });
    expect(await driver.modulesDetailSidebarLead()).toBe("A Ada Lovelace");
  });

  it("returns null when the sidebar, name, or trigger is absent", async () => {
    const noSidebar = harness({ [SIDEBAR]: { count: 0 } });
    expect(await noSidebar.driver.modulesDetailSidebarLead()).toBe(null);
    const noName = harness({ [`${ROW} css(span[role="img"])`]: { count: 1, attrs: {} } });
    expect(await noName.driver.modulesDetailSidebarLead()).toBe(null);
    const noTrigger = harness({
      [`${ROW} css(span[role="img"])`]: { count: 0 },
      [`${ROW} role(button,)`]: { count: 0 },
    });
    expect(await noTrigger.driver.modulesDetailSidebarLead()).toBe(null);
  });
});

describe("modulesDetailSidebarSetLead", () => {
  it("activates the row and option, then verifies the lead", async () => {
    const row = `${SIDEBAR} role(group,Lead)`;
    const { fake, driver } = harness({
      [`${row} css(span[role="img"])`]: { count: 1, attrs: { "aria-label": "Te Am" } },
    });
    await driver.modulesDetailSidebarSetLead("Te Am");
    expect(fake.clicks).toContain(`${row} role(button,)`);
    expect(fake.clicks).toContain("page role(option,Te Am)");
  });
});

describe("modulesDetailSidebarMembers", () => {
  const AVATARS = `${SIDEBAR} role(group,Members) css(span[role="img"])`;

  it("returns avatar names in order", async () => {
    const { driver } = harness({ [AVATARS]: { count: 1, attrs: { "aria-label": "Ada Lovelace" } } });
    expect(await driver.modulesDetailSidebarMembers()).toEqual(["Ada Lovelace"]);
  });

  it("returns empty when no avatar or name renders", async () => {
    const none = harness({ [AVATARS]: { count: 0 } });
    expect(await none.driver.modulesDetailSidebarMembers()).toEqual([]);
    const blank = harness({ [AVATARS]: { count: 1, attrs: {} } });
    expect(await blank.driver.modulesDetailSidebarMembers()).toEqual([]);
  });
});

describe("modulesDetailSidebarAddMember", () => {
  it("activates the row and option, then verifies membership", async () => {
    const row = `${SIDEBAR} role(group,Members)`;
    const { fake, driver } = harness({
      [`${row} css(span[role="img"])`]: { count: 1, attrs: { "aria-label": "Te Am" } },
    });
    await driver.modulesDetailSidebarAddMember("Te Am");
    expect(fake.clicks).toContain(`${row} role(button,)`);
    expect(fake.clicks).toContain("page role(option,Te Am)");
  });
});

describe("modulesDetailSidebarStatusDisabled", () => {
  it("reads the pill disabled flag", async () => {
    const table: Table = { [pillKey("Backlog")]: { count: 1, disabled: true } };
    for (const label of ["Planned", "In Progress", "Paused", "Completed", "Cancelled"]) {
      table[pillKey(label)] = { count: 0 };
    }
    expect(await harness(table).driver.modulesDetailSidebarStatusDisabled()).toBe(true);
    const enabled: Table = { [pillKey("Backlog")]: { count: 1, disabled: false } };
    for (const label of ["Planned", "In Progress", "Paused", "Completed", "Cancelled"]) {
      enabled[pillKey(label)] = { count: 0 };
    }
    expect(await harness(enabled).driver.modulesDetailSidebarStatusDisabled()).toBe(false);
  });

  it("refuses edits when no pill renders", async () => {
    const table: Table = {};
    for (const label of ["Backlog", "Planned", "In Progress", "Paused", "Completed", "Cancelled"]) {
      table[pillKey(label)] = { count: 0 };
    }
    expect(await harness(table).driver.modulesDetailSidebarStatusDisabled()).toBe(true);
  });
});

describe("modulesDetailSidebarLeadDisabled", () => {
  const key = `${SIDEBAR} role(group,Lead) role(button,)`;

  it("reads the lead trigger disabled flag", async () => {
    expect(await harness({ [key]: { disabled: true } }).driver.modulesDetailSidebarLeadDisabled()).toBe(true);
    expect(await harness({ [key]: { disabled: false } }).driver.modulesDetailSidebarLeadDisabled()).toBe(false);
  });
});

describe("modulesDetailSidebarMembersDisabled", () => {
  const key = `${SIDEBAR} role(group,Members) role(button,)`;

  it("reads the members trigger disabled flag", async () => {
    expect(await harness({ [key]: { disabled: true } }).driver.modulesDetailSidebarMembersDisabled()).toBe(true);
    expect(await harness({ [key]: { disabled: false } }).driver.modulesDetailSidebarMembersDisabled()).toBe(false);
  });
});

describe("modulesDetailSidebarWorkItemsText", () => {
  const key = `${SIDEBAR} role(group,Work items) css(span.text-body)`;

  it("reads the count text whitespace-collapsed", async () => {
    const { driver } = harness({ [key]: { count: 1, text: " 1/2 " } });
    expect(await driver.modulesDetailSidebarWorkItemsText()).toBe("1/2");
  });

  it("reads empty when the value is absent", async () => {
    const { driver } = harness({ [key]: { count: 0 } });
    expect(await driver.modulesDetailSidebarWorkItemsText()).toBe("");
  });
});

describe("modulesDetailSidebarPointsText", () => {
  const ROW = `${SIDEBAR} role(group,Points)`;

  it("reads the points count text", async () => {
    const { driver } = harness({ [`${ROW} css(span.text-body)`]: { count: 1, text: "0/2" } });
    expect(await driver.modulesDetailSidebarPointsText()).toBe("0/2");
  });

  it("reads null when the row or value is absent", async () => {
    const noRow = harness({ [ROW]: { count: 0 } });
    expect(await noRow.driver.modulesDetailSidebarPointsText()).toBe(null);
    const noValue = harness({ [ROW]: { count: 1 }, [`${ROW} css(span.text-body)`]: { count: 0 } });
    expect(await noValue.driver.modulesDetailSidebarPointsText()).toBe(null);
  });
});

describe("modulesDetailSlowFetchShowsLoader", () => {
  function runHandler(
    fake: FakePage,
    route: {
      request: () => { method: () => string };
      fulfill: (answer: unknown) => Promise<void>;
      continue: () => Promise<void>;
    }
  ): Promise<void> {
    const registered = fake.routes[0];
    if (registered === undefined) throw new Error("no route registered");
    return registered.handler(route);
  }

  it("delays module reads, reloads, and reports the watched skeleton", async () => {
    const { fake, driver } = harness();
    await expect(driver.modulesDetailSlowFetchShowsLoader()).resolves.toBe(true);
    expect(fake.routes).toHaveLength(1);
    expect(fake.routes[0]?.matcher).toBe("**/api/workspaces/**/modules**");
    expect(fake.pageCalls).toContain("reload");
  });

  it("delays GET reads and continues", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailSlowFetchShowsLoader();
    let continued = false;
    await runHandler(fake, {
      request: () => ({ method: () => "GET" }),
      fulfill: async () => undefined,
      continue: async () => {
        continued = true;
      },
    });
    expect(continued).toBe(true);
  });

  it("continues writes without delay", async () => {
    const { fake, driver } = harness();
    await driver.modulesDetailSlowFetchShowsLoader();
    let continued = false;
    await runHandler(fake, {
      request: () => ({ method: () => "POST" }),
      fulfill: async () => undefined,
      continue: async () => {
        continued = true;
      },
    });
    expect(continued).toBe(true);
  });

  it("rethrows a failed continue on an open page, swallows it once closed", async () => {
    const open = harness();
    await open.driver.modulesDetailSlowFetchShowsLoader();
    await expect(
      runHandler(open.fake, {
        request: () => ({ method: () => "GET" }),
        fulfill: async () => undefined,
        continue: async () => {
          throw new Error("gone");
        },
      })
    ).rejects.toThrow(/loader delay route failed/);
    const closed = harness();
    closed.fake.closed = true;
    await closed.driver.modulesDetailSlowFetchShowsLoader();
    await expect(
      runHandler(closed.fake, {
        request: () => ({ method: () => "GET" }),
        fulfill: async () => undefined,
        continue: async () => {
          throw new Error("gone");
        },
      })
    ).resolves.toBe(undefined);
  });
});
