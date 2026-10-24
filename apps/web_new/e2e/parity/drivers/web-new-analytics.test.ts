// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver analytics methods (NEWFRONT-731,
// ANL-001–005, ANL-008). A recording fake stands in for Playwright:
// each locator remembers the path of calls that built it, scripted
// answers stand in for the page, and a test asserts both which
// selector the driver used and what it does with the answer. Never
// about a real browser.

import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import type { ParityBrowserCookie } from "./parity-driver.js";
import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };
type Respond = (call: Call) => unknown;

const CHAIN = [
  "filter",
  "first",
  "last",
  "nth",
  "getByRole",
  "getByText",
  "getByPlaceholder",
  "locator",
  "getByTestId",
];
const ACTIONS = [
  "count",
  "click",
  "isVisible",
  "waitFor",
  "textContent",
  "allTextContents",
  "innerText",
  "getAttribute",
  "fill",
  "inputValue",
  "hover",
];

/** One stubbed route handler invocation. */
interface StubRoute {
  request: () => { method: () => string; url: () => string };
  fallback: () => Promise<void>;
}

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

class Recorder {
  readonly calls: Call[] = [];
  urlValue = "https://app.test/acme/analytics/overview";
  titleValue = "Acme Corp - Analytics";
  readonly waitForURLs: string[] = [];
  readonly routes: Array<{ pattern: unknown; handler: (route: StubRoute) => Promise<void> }> = [];
  readonly listeners: Array<{ event: string; handler: (...args: never[]) => void }> = [];
  evaluateImpl: (...args: never[]) => unknown = () => undefined;
  waitForFunctionImpl: (...args: never[]) => Promise<void> = async () => undefined;
  eventImpl: (event: string) => Promise<unknown> = async () => undefined;
  // Unstated paths answer with their type's quiet value, so a test only
  // has to describe the paths it cares about.
  respond: Respond = ({ method }) => {
    if (method === "count") return 0;
    if (method === "isVisible") return false;
    if (method === "allTextContents") return [];
    if (method === "textContent" || method === "inputValue" || method === "innerText") return "";
    if (method === "getAttribute") return null;
    return undefined;
  };

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => this.locator(`${path}.${method}(${render(args)})`);
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

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByText", "getByPlaceholder", "locator", "getByTestId"]) {
      node[method] = (...args: unknown[]) => this.locator(`page.${method}(${render(args)})`);
    }
    node.goto = async (...args: unknown[]) => {
      this.calls.push({ path: "page.goto", method: "goto", args });
    };
    node.waitForLoadState = async () => undefined;
    node.url = () => this.urlValue;
    node.title = async () => this.titleValue;
    node.waitForURL = async (url: string) => {
      this.waitForURLs.push(url);
    };
    node.waitForFunction = async (...args: never[]) => {
      this.calls.push({ path: "page.waitForFunction", method: "waitForFunction", args });
      return this.waitForFunctionImpl(...args);
    };
    node.waitForEvent = async (event: string) => {
      this.calls.push({ path: "page.waitForEvent", method: "waitForEvent", args: [event] });
      return this.eventImpl(event);
    };
    node.evaluate = async (...args: never[]) => {
      this.calls.push({ path: "page.evaluate", method: "evaluate", args });
      return this.evaluateImpl(...args);
    };
    node.route = async (pattern: unknown, handler: (route: StubRoute) => Promise<void>) => {
      this.routes.push({ pattern, handler });
    };
    node.on = (event: string, handler: (...args: never[]) => void) => {
      this.listeners.push({ event, handler });
    };
    node.off = (event: string, handler: (...args: never[]) => void) => {
      const at = this.listeners.findIndex((entry) => entry.event === event && entry.handler === handler);
      if (at >= 0) this.listeners.splice(at, 1);
    };
    node.context = () => ({
      addCookies: async (...args: unknown[]) => {
        this.calls.push({ path: "page.context.addCookies", method: "addCookies", args });
      },
    });
    node.keyboard = {
      press: async (key: string) => void this.calls.push({ path: "keyboard", method: "press", args: [key] }),
    };
    return node as unknown as Page;
  }

  /** Every action as `method <locator path>`, for readable assertions. */
  trace(): string[] {
    return this.calls.map((call) => `${call.method} ${call.path}`);
  }

  pressed(): string[] {
    return this.calls.filter((call) => call.path === "keyboard").map((call) => call.args[0] as string);
  }

  gotos(): string[] {
    return this.calls.filter((call) => call.path === "page.goto").map((call) => call.args[0] as string);
  }

  waitForFunctions(): unknown[][] {
    return this.calls.filter((call) => call.path === "page.waitForFunction").map((call) => call.args);
  }
}

/** Assert that the trace contains each fragment, in order. */
function expectTrace(actual: string[], expected: string[]): void {
  const joined = actual.join("\n");
  let from = 0;
  for (const fragment of expected) {
    const at = joined.indexOf(fragment, from);
    expect(at, `missing ${fragment} in:\n${joined}`).toBeGreaterThanOrEqual(0);
    from = at + fragment.length;
  }
}

const COOKIES: ParityBrowserCookie[] = [
  { name: "sessionid", value: "abc", domain: "app.test", path: "/", httpOnly: true, sameSite: "Lax" },
];

describe("analytics shell", () => {
  it("opens a tab authenticated and settles the shell", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsOpenTab("acme", "overview", COOKIES);
    expect(recorder.gotos()).toEqual(["/acme/analytics/overview"]);
    expectTrace(recorder.trace(), ["addCookies page.context.addCookies", "goto page.goto", "waitForFunction"]);
  });

  it("opens the bare address and waits out the redirect", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsOpenRoot("acme", COOKIES);
    expect(recorder.gotos()).toEqual(["/acme/analytics"]);
    expect(recorder.waitForURLs).toEqual(["**/analytics/**"]);
    expectTrace(recorder.trace(), ["waitForFunction"]);
  });

  it("throws when the shell settles on neither tabs nor the empty view", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsOpenTab("acme", "overview", COOKIES)).rejects.toThrow(
      "settled on neither tabs nor the empty view"
    );
  });

  it("reads the current path with its query", async () => {
    const recorder = new Recorder();
    recorder.urlValue = "https://app.test/acme/analytics/work-items?project_ids=a";
    await expect(new WebNewDriver(recorder.page()).analyticsCurrentPath()).resolves.toBe(
      "/acme/analytics/work-items?project_ids=a"
    );
  });

  it("reads tab ids in display order, skipping blanks", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count") return 3;
      if (method === "getAttribute" && path.endsWith(".nth(0)")) return "overview";
      if (method === "getAttribute" && path.endsWith(".nth(1)")) return "work-items";
      return null;
    };
    await expect(new WebNewDriver(recorder.page()).analyticsTabIds()).resolves.toEqual(["overview", "work-items"]);
    expectTrace(recorder.trace(), ['count page.getByRole("tab")']);
  });

  it("answers no tab ids when the strip is absent", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsTabIds()).resolves.toEqual([]);
  });

  it("reads the selected tab, or null when none is selected", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => "work-items";
    await expect(new WebNewDriver(recorder.page()).analyticsSelectedTab()).resolves.toBe("work-items");
    const unselected = new Recorder();
    unselected.evaluateImpl = () => null;
    await expect(new WebNewDriver(unselected.page()).analyticsSelectedTab()).resolves.toBeNull();
  });

  it("selects a tab and waits for its address", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    await new WebNewDriver(recorder.page()).analyticsSelectTab("work-items");
    expect(recorder.waitForURLs).toEqual(["**/analytics/work-items**"]);
    expectTrace(recorder.trace(), ['click page.getByTestId("analytics-tab-work-items")']);
  });

  it("throws when the tab has no trigger", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsSelectTab("nope")).rejects.toThrow(
      'analytics has no tab "nope"'
    );
  });

  it("reads panel visibility through the panel element", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => true;
    await expect(new WebNewDriver(recorder.page()).analyticsPanelVisible("overview")).resolves.toBe(true);
    const hidden = new Recorder();
    hidden.evaluateImpl = () => false;
    await expect(new WebNewDriver(hidden.page()).analyticsPanelVisible("work-items")).resolves.toBe(false);
  });

  it("reads the analytics breadcrumb, trimming blanks", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "allTextContents" ? [" Analytics ", ""] : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsBreadcrumb()).resolves.toEqual(["Analytics"]);
    expectTrace(recorder.trace(), [
      'allTextContents page.locator("[data-testid=\\"analytics-screen\\"] nav[aria-label=\\"Breadcrumb\\"] li")',
    ]);
  });

  it("reads the document title", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsPageTitle()).resolves.toBe("Acme Corp - Analytics");
  });

  it("answers null empty facts when the tabs view renders", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsEmptyFacts()).resolves.toBeNull();
  });

  it("reads the empty view facts after the action mounts", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    const facts = {
      title: "No project",
      description: "Create one.",
      actionLabel: "Create a project",
      actionDisabled: false,
    };
    recorder.evaluateImpl = () => facts;
    await expect(new WebNewDriver(recorder.page()).analyticsEmptyFacts()).resolves.toEqual(facts);
    expectTrace(recorder.trace(), ["waitFor", "evaluate page.evaluate"]);
  });

  it("clicks the empty view's creation action", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsEmptyCreateProject();
    expectTrace(recorder.trace(), ['click page.locator("[data-testid=\\"analytics-empty\\"]")']);
  });

  it("reads the enabled creation affordances", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => ["Create a project"];
    await expect(new WebNewDriver(recorder.page()).analyticsCreateAffordances()).resolves.toEqual(["Create a project"]);
  });
});

describe("analytics picker", () => {
  it("reads the trigger label, trimmed", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "innerText" ? "  Beta  " : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsPickerLabel()).resolves.toBe("Beta");
    expectTrace(recorder.trace(), [
      'waitFor page.getByTestId("analytics-picker-trigger")',
      'innerText page.getByTestId("analytics-picker-trigger")',
    ]);
  });

  it("leaves an open picker alone", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    await new WebNewDriver(recorder.page()).analyticsPickerOpen();
    expect(recorder.trace().filter((line) => line.startsWith("click"))).toEqual([]);
  });

  it("opens the picker onto its search box", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsPickerOpen();
    expectTrace(recorder.trace(), [
      'click page.getByTestId("analytics-picker-trigger")',
      'waitFor page.getByRole("listbox")',
      'waitFor page.getByRole("combobox"',
    ]);
  });

  it("leaves a closed picker alone when closing", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsPickerClose();
    expect(recorder.trace().filter((line) => line.startsWith("click"))).toEqual([]);
  });

  it("closes the picker through its trigger", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    await new WebNewDriver(recorder.page()).analyticsPickerClose();
    expectTrace(recorder.trace(), [
      'click page.getByTestId("analytics-picker-trigger")',
      "waitForFunction page.waitForFunction",
    ]);
    expect(recorder.pressed()).toEqual([]);
  });

  it("falls back to Escape when the trigger close stalls", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    let waits = 0;
    recorder.waitForFunctionImpl = async () => {
      waits += 1;
      if (waits === 1) throw new Error("timeout");
    };
    await new WebNewDriver(recorder.page()).analyticsPickerClose();
    expect(recorder.pressed()).toEqual(["Escape"]);
    expect(recorder.waitForFunctions()).toHaveLength(2);
  });

  it("reads the visible option names", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => ["Alpha", "Beta"];
    await expect(new WebNewDriver(recorder.page()).analyticsPickerOptions()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("searches through the picker's search box", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsPickerSearch("gam");
    expectTrace(recorder.trace(), ['fill page.getByRole("combobox"']);
    const fill = recorder.calls.find((call) => call.method === "fill");
    expect(fill?.args).toEqual(["gam", { timeout: 30_000 }]);
  });

  it("throws when the option is missing", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsPickerToggle("Gamma")).rejects.toThrow(
      'analytics picker has no option named "Gamma"'
    );
  });

  it("toggles an option and waits for its state to flip on", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "getAttribute") return "false";
      return undefined;
    };
    await new WebNewDriver(recorder.page()).analyticsPickerToggle("Beta");
    expectTrace(recorder.trace(), ["click", "waitForFunction page.waitForFunction"]);
    expect(recorder.waitForFunctions()[0]?.[1]).toEqual({ name: "Beta", selected: true });
  });

  it("toggles an option off when it starts selected", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "getAttribute") return "true";
      return undefined;
    };
    await new WebNewDriver(recorder.page()).analyticsPickerToggle("Beta");
    expect(recorder.waitForFunctions()[0]?.[1]).toEqual({ name: "Beta", selected: false });
  });

  it("throws when the toggle never settles", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsPickerToggle("Beta")).rejects.toThrow(
      'analytics picker toggle never settled for "Beta"'
    );
  });

  it("reads the checked option names", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => ["Beta"];
    await expect(new WebNewDriver(recorder.page()).analyticsPickerChecked()).resolves.toEqual(["Beta"]);
  });
});

describe("analytics request spy", () => {
  function emit(recorder: Recorder, url: string): void {
    for (const listener of recorder.listeners) {
      if (listener.event === "request") listener.handler({ url: () => url } as never);
    }
  }

  it("captures analytics reads with path and query", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsStartRequestSpy();
    emit(recorder, "https://app.test/api/workspaces/acme/advance-analytics/?tab=overview&project_ids=b");
    emit(recorder, "https://app.test/api/workspaces/acme/projects/");
    await expect(driver.analyticsSpyRequests()).resolves.toEqual([
      { path: "/api/workspaces/acme/advance-analytics/", query: "tab=overview&project_ids=b" },
    ]);
    await driver.analyticsStopRequestSpy();
  });

  it("restarts clean and unregisters on stop", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsStartRequestSpy();
    emit(recorder, "https://app.test/api/workspaces/acme/advance-analytics/?tab=overview");
    await driver.analyticsStartRequestSpy();
    await expect(driver.analyticsSpyRequests()).resolves.toEqual([]);
    expect(recorder.listeners.filter((listener) => listener.event === "request")).toHaveLength(1);
    await driver.analyticsStopRequestSpy();
    expect(recorder.listeners.filter((listener) => listener.event === "request")).toHaveLength(0);
    await driver.analyticsStopRequestSpy();
  });
});

describe("analytics headline cards", () => {
  it("reads the settled overview cards in order", async () => {
    const recorder = new Recorder();
    const cards = [
      { label: "Total Users", value: "3" },
      { label: "Total Admins", value: "1" },
    ];
    recorder.evaluateImpl = () => cards;
    await expect(new WebNewDriver(recorder.page()).analyticsOverviewCards()).resolves.toEqual(cards);
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
  });

  it("reads the settled work-items cards in order", async () => {
    const recorder = new Recorder();
    const cards = [{ label: "Total Work items", value: "4" }];
    recorder.evaluateImpl = () => cards;
    await expect(new WebNewDriver(recorder.page()).analyticsWorkItemsCards()).resolves.toEqual(cards);
  });

  it("throws when the cards never settle", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsOverviewCards()).rejects.toThrow(
      "analytics headline cards never settled"
    );
  });

  it("counts the visible placeholders", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => 11;
    await expect(new WebNewDriver(recorder.page()).analyticsHeadlinePlaceholders()).resolves.toBe(11);
  });
});

describe("analytics totals stall", () => {
  function stubRoute(url: string, method = "GET"): StubRoute & { fellThrough: () => boolean } {
    let fellThrough = false;
    return {
      request: () => ({ method: () => method, url: () => url }),
      fallback: async () => {
        fellThrough = true;
      },
      fellThrough: () => fellThrough,
    };
  }

  function installedHook(recorder: Recorder): (route: StubRoute) => Promise<void> {
    const [installed] = recorder.routes;
    if (!installed) throw new Error("expected the totals stall to install its route hook");
    return installed.handler;
  }

  it("installs one route hook no matter how often it stalls", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    await driver.analyticsTotalsStall("work-items");
    expect(recorder.routes).toHaveLength(1);
    expect(recorder.routes[0]?.pattern).toBe("**/api/workspaces/**");
  });

  it("holds a stalled totals read until release", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    const route = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=overview");
    let settled = false;
    const pending = installedHook(recorder)(route).then(() => {
      settled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(false);
    expect(route.fellThrough()).toBe(false);
    await driver.analyticsTotalsRelease();
    await pending;
    expect(settled).toBe(true);
    expect(route.fellThrough()).toBe(true);
  });

  it("holds one stalled read per tab and releases every gate", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    await driver.analyticsTotalsStall("work-items");
    const first = stubRoute("https://app.test/api/workspaces/acme/advance-analytics?tab=overview");
    const second = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=work-items");
    let settled = 0;
    const hook = installedHook(recorder);
    const pending = Promise.all([hook(first).then(() => (settled += 1)), hook(second).then(() => (settled += 1))]);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(0);
    await driver.analyticsTotalsRelease();
    await pending;
    expect(settled).toBe(2);
    // A later stall starts clean: the released gates do not linger.
    await driver.analyticsTotalsStall("overview");
    const third = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=overview");
    let thirdSettled = false;
    const thirdPending = installedHook(recorder)(third).then(() => {
      thirdSettled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(thirdSettled).toBe(false);
    await driver.analyticsTotalsRelease();
    await thirdPending;
    expect(thirdSettled).toBe(true);
  });

  it("falls through reads outside the stall", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    const hook = installedHook(recorder);
    const otherTab = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=work-items");
    await hook(otherTab);
    expect(otherTab.fellThrough()).toBe(true);
    const otherPath = stubRoute("https://app.test/api/workspaces/acme/projects/");
    await hook(otherPath);
    expect(otherPath.fellThrough()).toBe(true);
    const post = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=overview", "POST");
    await hook(post);
    expect(post.fellThrough()).toBe(true);
    const unparseable = stubRoute("http://[");
    await hook(unparseable);
    expect(unparseable.fellThrough()).toBe(true);
  });
});

describe("analytics charts stall", () => {
  function stubRoute(url: string, method = "GET"): StubRoute & { fellThrough: () => boolean } {
    let fellThrough = false;
    return {
      request: () => ({ method: () => method, url: () => url }),
      fallback: async () => {
        fellThrough = true;
      },
      fellThrough: () => fellThrough,
    };
  }

  function installedHook(recorder: Recorder): (route: StubRoute) => Promise<void> {
    const [installed] = recorder.routes;
    if (!installed) throw new Error("expected the charts stall to install its route hook");
    return installed.handler;
  }

  it("shares one route hook with the totals stall", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    await driver.analyticsChartsStall("projects");
    await driver.analyticsChartsStall("work-items");
    expect(recorder.routes).toHaveLength(1);
    expect(recorder.routes[0]?.pattern).toBe("**/api/workspaces/**");
  });

  it("holds a stalled charts read until release", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsChartsStall("projects");
    const route = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts/?type=projects");
    let settled = false;
    const pending = installedHook(recorder)(route).then(() => {
      settled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(false);
    expect(route.fellThrough()).toBe(false);
    await driver.analyticsChartsRelease();
    await pending;
    expect(settled).toBe(true);
    expect(route.fellThrough()).toBe(true);
  });

  it("holds one stalled read per chart kind and releases every gate", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsChartsStall("projects");
    await driver.analyticsChartsStall("work-items");
    const first = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts?type=projects");
    const second = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts/?type=work-items");
    let settled = 0;
    const hook = installedHook(recorder);
    const pending = Promise.all([hook(first).then(() => (settled += 1)), hook(second).then(() => (settled += 1))]);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(0);
    await driver.analyticsChartsRelease();
    await pending;
    expect(settled).toBe(2);
  });

  it("falls through reads outside the stall", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsChartsStall("projects");
    const hook = installedHook(recorder);
    const otherType = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts/?type=work-items");
    await hook(otherType);
    expect(otherType.fellThrough()).toBe(true);
    const otherPath = stubRoute("https://app.test/api/workspaces/acme/advance-analytics/?tab=overview");
    await hook(otherPath);
    expect(otherPath.fellThrough()).toBe(true);
    const post = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts/?type=projects", "POST");
    await hook(post);
    expect(post.fellThrough()).toBe(true);
  });
});

describe("analytics project-stats stall", () => {
  function stubRoute(url: string, method = "GET"): StubRoute & { fellThrough: () => boolean } {
    let fellThrough = false;
    return {
      request: () => ({ method: () => method, url: () => url }),
      fallback: async () => {
        fellThrough = true;
      },
      fellThrough: () => fellThrough,
    };
  }

  function installedHook(recorder: Recorder): (route: StubRoute) => Promise<void> {
    const [installed] = recorder.routes;
    if (!installed) throw new Error("expected the stats stall to install its route hook");
    return installed.handler;
  }

  it("shares one route hook with the other stalls", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsTotalsStall("overview");
    await driver.analyticsChartsStall("projects");
    await driver.analyticsProjectStatsStall();
    expect(recorder.routes).toHaveLength(1);
  });

  it("holds a stalled stats read until release", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsProjectStatsStall();
    const route = stubRoute("https://app.test/api/workspaces/acme/project-stats/?fields=total_issues,completed_issues");
    let settled = false;
    const pending = installedHook(recorder)(route).then(() => {
      settled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(false);
    expect(route.fellThrough()).toBe(false);
    await driver.analyticsProjectStatsRelease();
    await pending;
    expect(settled).toBe(true);
    expect(route.fellThrough()).toBe(true);
    // A later stall starts clean: the released gates do not linger.
    await driver.analyticsProjectStatsStall();
    const second = stubRoute("https://app.test/api/workspaces/acme/project-stats/");
    let secondSettled = false;
    const secondPending = installedHook(recorder)(second).then(() => {
      secondSettled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(secondSettled).toBe(false);
    await driver.analyticsProjectStatsRelease();
    await secondPending;
    expect(secondSettled).toBe(true);
  });

  it("falls through reads outside the stall", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsProjectStatsStall();
    const hook = installedHook(recorder);
    const otherPath = stubRoute("https://app.test/api/workspaces/acme/advance-analytics-charts/?type=projects");
    await hook(otherPath);
    expect(otherPath.fellThrough()).toBe(true);
    const post = stubRoute("https://app.test/api/workspaces/acme/project-stats/", "POST");
    await hook(post);
    expect(post.fellThrough()).toBe(true);
  });

  it("falls the stats read through once released", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsProjectStatsStall();
    await driver.analyticsProjectStatsRelease();
    const route = stubRoute("https://app.test/api/workspaces/acme/project-stats/");
    await installedHook(recorder)(route);
    expect(route.fellThrough()).toBe(true);
  });
});

describe("analytics radar", () => {
  it("reads the settled summary rows in order", async () => {
    const recorder = new Recorder();
    const rows = [
      { dimension: "Work Items", count: "2" },
      { dimension: "Cycles", count: "0" },
    ];
    recorder.evaluateImpl = () => rows;
    await expect(new WebNewDriver(recorder.page()).analyticsRadarSummary()).resolves.toEqual(rows);
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
  });

  it("throws when the radar section never settles", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsRadarSummary()).rejects.toThrow(
      "analytics radar section never settled"
    );
  });

  it("reads the radar facts", async () => {
    const recorder = new Recorder();
    const facts = { dimensions: ["Work Items", "Cycles"], plotted: true };
    recorder.evaluateImpl = () => facts;
    await expect(new WebNewDriver(recorder.page()).analyticsRadarFacts()).resolves.toEqual(facts);
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
  });

  it("reads whether the skeleton shows", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => true;
    await expect(new WebNewDriver(recorder.page()).analyticsRadarLoading()).resolves.toBe(true);
    const settled = new Recorder();
    settled.evaluateImpl = () => false;
    await expect(new WebNewDriver(settled.page()).analyticsRadarLoading()).resolves.toBe(false);
  });
});

describe("analytics active projects", () => {
  it("reads the settled rows in order", async () => {
    const recorder = new Recorder();
    const rows = [
      { name: "Alpha", badge: "100%", tone: "success", truncated: true },
      { name: "Beta", badge: "50%", tone: "danger", truncated: false },
    ];
    recorder.evaluateImpl = () => rows;
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjects()).resolves.toEqual(rows);
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
  });

  it("throws when the list never settles", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjects()).rejects.toThrow(
      "analytics active-projects list never settled"
    );
  });

  it("reads whether placeholder rows show", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => true;
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjectsLoading()).resolves.toBe(true);
    const settled = new Recorder();
    settled.evaluateImpl = () => false;
    await expect(new WebNewDriver(settled.page()).analyticsActiveProjectsLoading()).resolves.toBe(false);
  });

  it("hovers the matching row and reads its tooltip", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count") return 2;
      if (method === "innerText" && path.includes(".nth(0)")) return "Alpha";
      if (method === "innerText" && path.includes(".nth(1)")) return "Beta Project";
      return undefined;
    };
    recorder.evaluateImpl = () => "Beta Project";
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjectTooltip("Beta Project")).resolves.toBe(
      "Beta Project"
    );
    expectTrace(recorder.trace(), [
      'count page.locator("[data-testid=\\"analytics-active-row\\"]")',
      "hover",
      "waitForFunction page.waitForFunction",
      "evaluate page.evaluate",
    ]);
    const hovers = recorder.calls.filter((call) => call.method === "hover");
    expect(hovers).toHaveLength(1);
    expect(hovers[0]?.path).toContain(".nth(1)");
  });

  it("answers null when no row matches", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "innerText") return "Alpha";
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjectTooltip("Missing")).resolves.toBeNull();
    expect(recorder.trace().filter((line) => line.startsWith("hover"))).toEqual([]);
    expect(recorder.trace().filter((line) => line.startsWith("evaluate"))).toEqual([]);
  });

  it("answers null when no rows render", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjectTooltip("Alpha")).resolves.toBeNull();
  });

  it("answers null when the tooltip never appears", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "innerText") return "Alpha";
      return undefined;
    };
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    recorder.evaluateImpl = () => null;
    await expect(new WebNewDriver(recorder.page()).analyticsActiveProjectTooltip("Alpha")).resolves.toBeNull();
  });
});

describe("analytics section guidance", () => {
  it("reads the guidance text of a known section", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => "Nothing to plot yet. Add work to see it here.";
    await expect(new WebNewDriver(recorder.page()).analyticsSectionGuidance("Project Insights")).resolves.toBe(
      "Nothing to plot yet. Add work to see it here."
    );
    const evaluated = recorder.calls.find((call) => call.method === "evaluate");
    expect(evaluated?.args[1]).toBe("analytics-radar");
  });

  it("maps the created-vs-resolved section to its testid", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => "Create projects to start tracking here.";
    await expect(new WebNewDriver(recorder.page()).analyticsSectionGuidance("Created vs Resolved")).resolves.toBe(
      "Create projects to start tracking here."
    );
    const evaluated = recorder.calls.find((call) => call.method === "evaluate");
    expect(evaluated?.args[1]).toBe("analytics-created");
  });

  it("answers null for an unknown section without touching the page", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsSectionGuidance("No Such Section")).resolves.toBeNull();
    expect(recorder.trace()).toEqual([]);
  });
});

describe("analytics created vs resolved", () => {
  it("reads the settled facts with the guidance reader", async () => {
    const recorder = new Recorder();
    const plotted = { legend: ["Resolved", "Created"], xLabels: ["Oct 01, 2026"], series: 2 };
    recorder.evaluateImpl = (...args: never[]) => (args.length > 1 ? null : plotted);
    await expect(new WebNewDriver(recorder.page()).analyticsCreatedResolvedFacts()).resolves.toEqual({
      ...plotted,
      guidance: null,
    });
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
    expect(recorder.calls.filter((call) => call.method === "evaluate")).toHaveLength(2);
  });

  it("throws when the section never settles", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsCreatedResolvedFacts()).rejects.toThrow(
      "analytics created-vs-resolved section never settled"
    );
  });

  it("reads whether the loader shows", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => true;
    await expect(new WebNewDriver(recorder.page()).analyticsCreatedResolvedLoading()).resolves.toBe(true);
    const settled = new Recorder();
    settled.evaluateImpl = () => false;
    await expect(new WebNewDriver(settled.page()).analyticsCreatedResolvedLoading()).resolves.toBe(false);
  });
});

describe("analytics custom pickers", () => {
  it("reads the three trigger labels", async () => {
    const recorder = new Recorder();
    const labels = { metric: "Work item", xAxis: "Priority", split: "Add Property" };
    recorder.evaluateImpl = () => labels;
    await expect(new WebNewDriver(recorder.page()).analyticsCustomPickerLabels()).resolves.toEqual(labels);
    expectTrace(recorder.trace(), ["evaluate page.evaluate"]);
  });

  it("opens a picker, reads its options, and closes it", async () => {
    const recorder = new Recorder();
    recorder.evaluateImpl = () => ["No value", "Priority"];
    await expect(new WebNewDriver(recorder.page()).analyticsCustomPickerOptions(2)).resolves.toEqual([
      "No value",
      "Priority",
    ]);
    expectTrace(recorder.trace(), [
      'click page.getByTestId("analytics-custom-trigger-2")',
      "waitForFunction page.waitForFunction",
      "evaluate page.evaluate",
      "press keyboard",
      "waitForFunction page.waitForFunction",
    ]);
    expect(recorder.pressed()).toEqual(["Escape"]);
    expect(recorder.waitForFunctions().map((args) => args[1])).toEqual([true, false]);
  });

  it("throws when the picker never opens", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsCustomPickerOptions(1)).rejects.toThrow(
      "custom picker 1 never opened"
    );
  });

  it("selects the matching option and waits out the re-query", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count") return 2;
      if (method === "innerText" && path.includes(".nth(0)")) return "Priority";
      if (method === "innerText" && path.includes(".nth(1)")) return "State name";
      return undefined;
    };
    await new WebNewDriver(recorder.page()).analyticsCustomPickerSelect(2, "State name");
    expectTrace(recorder.trace(), [
      'click page.getByTestId("analytics-custom-trigger-2")',
      "waitForFunction page.waitForFunction",
      "count",
      "click",
      "waitForFunction page.waitForFunction",
      "waitForFunction page.waitForFunction",
      "waitForFunction page.waitForFunction",
    ]);
    const clicks = recorder.calls.filter((call) => call.method === "click");
    expect(clicks).toHaveLength(2);
    expect(clicks[1]?.path).toContain(".nth(1)");
    const waits = recorder.waitForFunctions();
    expect(waits.map((args) => args[1])).toEqual([true, false, { index: 2, label: "State name" }, { timeout: 30000 }]);
  });

  it("maps the clear row back to the empty-split label", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count") return 1;
      if (method === "innerText" && path.includes(".nth(0)")) return "No value";
      return undefined;
    };
    await new WebNewDriver(recorder.page()).analyticsCustomPickerSelect(2, "No value");
    const waits = recorder.waitForFunctions();
    expect(waits.map((args) => args[1])).toEqual([
      true,
      false,
      { index: 2, label: "Add Property" },
      { timeout: 30000 },
    ]);
  });

  it("throws when the option is missing", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "innerText") return "Priority";
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).analyticsCustomPickerSelect(1, "Missing")).rejects.toThrow(
      'custom picker 1 has no "Missing" option'
    );
  });

  it("throws when the pick never settles on its label", async () => {
    const recorder = new Recorder();
    let waits = 0;
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "innerText") return "Label";
      return undefined;
    };
    recorder.waitForFunctionImpl = async () => {
      waits += 1;
      if (waits === 3) throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsCustomPickerSelect(1, "Label")).rejects.toThrow(
      'custom picker 1 never settled on "Label"'
    );
  });
});

describe("analytics custom chart", () => {
  it("reads the settled chart facts", async () => {
    const recorder = new Recorder();
    const facts = {
      segments: [{ cx: 10, cy: 20, fill: "#ef4444", topRounded: true, bottomRounded: true }],
      xLabels: ["Urgent"],
      legend: false,
    };
    recorder.evaluateImpl = () => facts;
    await expect(new WebNewDriver(recorder.page()).analyticsCustomChartFacts()).resolves.toEqual(facts);
    expectTrace(recorder.trace(), ["waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
  });

  it("throws when the section never settles", async () => {
    const recorder = new Recorder();
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(recorder.page()).analyticsCustomChartFacts()).rejects.toThrow(
      "analytics custom-insights section never settled"
    );
  });

  it("hovers one bar and reads its tooltip segments", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 3;
      return undefined;
    };
    const rows = [
      { label: "Backlog", value: "1" },
      { label: "Done", value: "1" },
    ];
    recorder.evaluateImpl = () => rows;
    await expect(new WebNewDriver(recorder.page()).analyticsCustomTooltipSegments(0)).resolves.toEqual(rows);
    expectTrace(recorder.trace(), ["count", "hover", "waitForFunction page.waitForFunction", "evaluate page.evaluate"]);
    const hovers = recorder.calls.filter((call) => call.method === "hover");
    expect(hovers).toHaveLength(1);
    expect(hovers[0]?.path).toContain(".nth(0)");
  });

  it("answers no rows when the bar is missing", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsCustomTooltipSegments(0)).resolves.toEqual([]);
    expect(recorder.trace().filter((line) => line.startsWith("hover"))).toEqual([]);
    expect(recorder.trace().filter((line) => line.startsWith("evaluate"))).toEqual([]);
  });

  it("answers no rows when the tooltip never appears", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => {
      if (method === "count") return 1;
      return undefined;
    };
    recorder.waitForFunctionImpl = async () => {
      throw new Error("timeout");
    };
    recorder.evaluateImpl = () => [];
    await expect(new WebNewDriver(recorder.page()).analyticsCustomTooltipSegments(0)).resolves.toEqual([]);
  });
});

describe("analytics export tables", () => {
  const CUSTOM = `page.getByTestId("analytics-custom-table")`;
  const INSIGHT = `page.getByTestId("analytics-breakdown-table")`;

  it("opens the work-items tab and settles both table sections", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).analyticsExportOpen("acme", COOKIES);
    expect(recorder.gotos()).toEqual(["/acme/analytics/work-items"]);
    expectTrace(recorder.trace(), [
      "addCookies page.context.addCookies",
      'waitFor page.getByTestId("analytics-custom-table")',
      'waitFor page.getByTestId("analytics-breakdown-table")',
    ]);
  });

  it("counts the export actions across both tables", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 2 : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsExportButtonCount()).resolves.toBe(2);
    expect(recorder.trace()).toContain('count page.getByRole("button",{"name":"Export as csv"})');
  });

  it("addresses the chart table for chart facts and the insight table otherwise", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 3 : undefined);
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.analyticsExportRowCount("chart")).resolves.toBe(3);
    await expect(driver.analyticsExportRowCount("insight")).resolves.toBe(3);
    expectTrace(recorder.trace(), [`count ${CUSTOM}.locator("tbody tr")`, `count ${INSIGHT}.locator("tbody tr")`]);
  });

  it("trims display headers", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "allTextContents" ? [" Project ", "Backlog"] : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsExportHeaders("insight")).resolves.toEqual([
      "Project",
      "Backlog",
    ]);
    expect(recorder.trace()).toContain(`allTextContents ${INSIGHT}.locator("thead th")`);
  });

  it("trims the row-count label, answering empty when the label is missing", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "textContent" ? "  2 Projects  " : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsExportCountLabel("insight")).resolves.toBe("2 Projects");
    const nullLabel = new Recorder();
    nullLabel.respond = ({ method }) => (method === "textContent" ? null : undefined);
    await expect(new WebNewDriver(nullLabel.page()).analyticsExportCountLabel("insight")).resolves.toBe("");
  });

  it("reports whether the search box is open", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "isVisible" ? true : undefined);
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.analyticsExportSearchExpanded("chart")).resolves.toBe(true);
    const closed = new WebNewDriver(new Recorder().page());
    await expect(closed.analyticsExportSearchExpanded("insight")).resolves.toBe(false);
  });

  it("expands a collapsed search box before filling it", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "isVisible" ? false : undefined);
    await new WebNewDriver(recorder.page()).analyticsExportSearch("insight", "alpha");
    expectTrace(recorder.trace(), [
      `isVisible ${INSIGHT}.getByPlaceholder("Search")`,
      `click ${INSIGHT}.getByRole("button",{"name":"Search","exact":true})`,
      `waitFor ${INSIGHT}.getByPlaceholder("Search")`,
      `fill ${INSIGHT}.getByPlaceholder("Search")`,
    ]);
    expect(recorder.calls.find((call) => call.method === "fill")?.args).toEqual(["alpha", { timeout: 30_000 }]);
  });

  it("fills an already-open search box without clicking", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "isVisible" ? true : undefined);
    await new WebNewDriver(recorder.page()).analyticsExportClearSearch("chart");
    expect(recorder.trace().filter((line) => line.startsWith("click"))).toEqual([]);
    expect(recorder.trace()).toContain(`fill ${CUSTOM}.getByPlaceholder("Search")`);
  });

  it("closes the search box and waits for it to hide", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "isVisible" ? true : undefined);
    await new WebNewDriver(recorder.page()).analyticsExportSearchClose("chart");
    expectTrace(recorder.trace(), [
      `isVisible ${CUSTOM}.getByPlaceholder("Search")`,
      `click ${CUSTOM}.getByRole("button",{"name":"Close search","exact":true})`,
      `waitFor ${CUSTOM}.getByPlaceholder("Search")`,
    ]);
    const waits = recorder.calls.filter((call) => call.method === "waitFor");
    expect(waits[0]?.args).toEqual([{ state: "hidden", timeout: 30_000 }]);
  });

  it("refuses to close an already-closed search box", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).analyticsExportSearchClose("insight")).rejects.toThrow(
      "search box is already closed"
    );
  });

  it("reads collapsed guidance only from a one-row, one-cell body", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count" && path.endsWith(`locator("tbody tr")`)) return 1;
      if (method === "count") return 1;
      if (method === "textContent") return "  Create projects with work items.  ";
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).analyticsExportEmptyText("insight")).resolves.toBe(
      "Create projects with work items."
    );
    expectTrace(recorder.trace(), [
      `count ${INSIGHT}.locator("tbody tr")`,
      `count ${INSIGHT}.locator("tbody tr").first().locator("td")`,
      `textContent ${INSIGHT}.locator("tbody tr").first().locator("td").first()`,
    ]);
  });

  it("answers no guidance for a data table, a multi-cell row, or a short note", async () => {
    const data = new Recorder();
    data.respond = ({ method, path }) => (method === "count" && path.endsWith(`locator("tbody tr")`) ? 2 : undefined);
    await expect(new WebNewDriver(data.page()).analyticsExportEmptyText("insight")).resolves.toBeNull();

    const manyCells = new Recorder();
    manyCells.respond = ({ method, path }) => {
      if (method !== "count") return undefined;
      return path.endsWith(`locator("tbody tr")`) ? 1 : 2;
    };
    await expect(new WebNewDriver(manyCells.page()).analyticsExportEmptyText("insight")).resolves.toBeNull();

    const short = new Recorder();
    short.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "textContent") return "Short";
      return undefined;
    };
    await expect(new WebNewDriver(short.page()).analyticsExportEmptyText("chart")).resolves.toBeNull();

    const nullText = new Recorder();
    nullText.respond = ({ method }) => {
      if (method === "count") return 1;
      if (method === "textContent") return null;
      return undefined;
    };
    await expect(new WebNewDriver(nullText.page()).analyticsExportEmptyText("chart")).resolves.toBeNull();
  });

  it("counts the insight skeleton cells", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 30 : undefined);
    await expect(new WebNewDriver(recorder.page()).analyticsInsightSkeletonCells()).resolves.toBe(30);
    expect(recorder.trace()).toContain(`count ${INSIGHT}.locator("td > span[style*='height'][style*='20px']")`);
  });

  it("stalls and releases the insight stats read through the shared hook", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.analyticsInsightTableStall();
    expect(recorder.routes).toHaveLength(1);
    let fellThrough = false;
    const route = {
      request: () => ({
        method: () => "GET",
        url: () => "https://app.test/api/workspaces/acme/advance-analytics-stats/",
      }),
      fallback: async () => {
        fellThrough = true;
      },
    };
    let settled = false;
    const pending = recorder.routes[0]!.handler(route).then(() => {
      settled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(false);
    await driver.analyticsInsightTableRelease();
    await pending;
    expect(settled).toBe(true);
    expect(fellThrough).toBe(true);

    // Both accepted spellings address the same insight stats path.
    await driver.analyticsInsightTableStall();
    const withoutSlash = {
      request: () => ({
        method: () => "GET",
        url: () => "https://app.test/api/workspaces/acme/advance-analytics-stats",
      }),
      fallback: async () => undefined,
    };
    let secondSettled = false;
    const second = recorder.routes[0]!.handler(withoutSlash).then(() => {
      secondSettled = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(secondSettled).toBe(false);
    await driver.analyticsInsightTableRelease();
    await second;
    expect(secondSettled).toBe(true);
  });

  it("reads every row's cells, trimming each", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count" && path.endsWith('.locator("tbody tr")')) return 2;
      if (method === "allTextContents") {
        return path.includes(".nth(0)") ? [" Alpha ", "2"] : ["Beta", "4"];
      }
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).analyticsExportRowCells("insight")).resolves.toEqual([
      ["Alpha", "2"],
      ["Beta", "4"],
    ]);
  });

  it("captures a download with its name and contents", async () => {
    const recorder = new Recorder();
    recorder.eventImpl = async () => ({
      path: async () => "/tmp/acme-analytics.csv",
      suggestedFilename: () => "acme-analytics.csv",
    });
    const driver = new WebNewDriver(recorder.page());
    const reads: string[] = [];
    (driver as unknown as { readDownload: (path: string) => string }).readDownload = (path) => {
      reads.push(path);
      return "\uFEFFPriority,Count\r\nUrgent,1\r\n";
    };
    await expect(driver.analyticsExportDownload("chart")).resolves.toEqual({
      fileName: "acme-analytics.csv",
      content: "\uFEFFPriority,Count\r\nUrgent,1\r\n",
    });
    expect(reads).toEqual(["/tmp/acme-analytics.csv"]);
    expectTrace(recorder.trace(), [`click ${CUSTOM}.getByRole("button",{"name":"Export as csv"})`]);
  });

  it("rejects a download that arrived without a path", async () => {
    const recorder = new Recorder();
    recorder.eventImpl = async () => ({ path: async () => null, suggestedFilename: () => "x.csv" });
    await expect(new WebNewDriver(recorder.page()).analyticsExportDownload("insight")).rejects.toThrow(
      "landed without a path"
    );
  });

  it("reports no download when the filter suppressed it", async () => {
    const quiet = new Recorder();
    quiet.eventImpl = async () => {
      throw new Error("timeout");
    };
    await expect(new WebNewDriver(quiet.page()).analyticsExportYieldsNoDownload("insight")).resolves.toBe(true);

    const fired = new Recorder();
    fired.eventImpl = async () => ({});
    await expect(new WebNewDriver(fired.page()).analyticsExportYieldsNoDownload("chart")).resolves.toBe(false);
  });
});
