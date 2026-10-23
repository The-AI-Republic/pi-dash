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
  "waitFor",
  "textContent",
  "allTextContents",
  "innerText",
  "getAttribute",
  "fill",
  "inputValue",
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
  // Unstated paths answer with their type's quiet value, so a test only
  // has to describe the paths it cares about.
  respond: Respond = ({ method }) => {
    if (method === "count") return 0;
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
