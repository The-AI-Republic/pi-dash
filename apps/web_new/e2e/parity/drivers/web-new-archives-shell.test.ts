// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archives shell driver family (NEWFRONT-698,
// ARCH-001..003). A stub Page replays settled shells, slow paints,
// stuck loaders and recorded requests here instead of a browser; the
// browser-serialized predicates are captured and invoked against jsdom
// so every shipped line runs.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { archivesSkeletonObserverBoot, WebNewDriver } from "./web-new";

function loc(
  overrides: {
    count?: () => Promise<number>;
    innerText?: () => Promise<string>;
    isVisible?: () => Promise<boolean>;
    waitFor?: (options?: unknown) => Promise<void>;
    click?: (options?: unknown) => Promise<void>;
    hover?: (options?: unknown) => Promise<void>;
    scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
    nested?: (selector: string) => Locator;
    byTestId?: (id: string) => Locator;
  } = {}
): Locator {
  const node: Record<string, unknown> = {};
  node["count"] = overrides.count ?? (async () => 1);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["hover"] = overrides.hover ?? (async () => undefined);
  node["scrollIntoViewIfNeeded"] = overrides.scrollIntoViewIfNeeded ?? (async () => undefined);
  const self = node as unknown as Locator;
  node["first"] = () => self;
  node["nth"] = () => self;
  node["locator"] = (selector: string) => overrides.nested?.(selector) ?? self;
  node["getByTestId"] = (id: string) => overrides.byTestId?.(id) ?? self;
  return self;
}

interface RouteCall {
  matcher: unknown;
  handler: (route: Route) => Promise<void>;
}

interface PageCalls {
  goto: string[];
  reloads: number;
  timeouts: number[];
  routes: RouteCall[];
  evaluates: Array<{ fn: (arg: never) => unknown; arg: unknown }>;
  waitForFunctions: Array<{ fn: (arg: never) => unknown; arg: unknown }>;
  waitForURLs: string[];
  initScripts: unknown[];
  clicks: string[];
}

function stubPage(config: {
  url?: string | (() => string);
  title?: string;
  locatorFor?: (selector: string) => Locator;
  roleFor?: (role: string, options?: unknown) => Locator;
  textFor?: (text: string) => Locator;
  evaluateImpl?: (fn: (arg: never) => unknown, arg: unknown) => unknown;
}): { page: Page; calls: PageCalls } {
  const calls: PageCalls = {
    goto: [],
    reloads: 0,
    timeouts: [],
    routes: [],
    evaluates: [],
    waitForFunctions: [],
    waitForURLs: [],
    initScripts: [],
    clicks: [],
  };
  let now = 1_000_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  const page = {
    goto: async (url: string) => {
      calls.goto.push(url);
    },
    reload: async () => {
      calls.reloads += 1;
    },
    waitForLoadState: async () => undefined,
    waitForTimeout: async (ms: number) => {
      calls.timeouts.push(ms);
      now += ms;
    },
    url: () => (typeof config.url === "function" ? config.url() : (config.url ?? "http://new.test/")),
    title: async () => config.title ?? "",
    locator: (selector: string) => config.locatorFor?.(selector) ?? loc(),
    getByRole: (role: string, options?: unknown) => config.roleFor?.(role, options) ?? loc(),
    getByText: (text: string) => config.textFor?.(text) ?? loc(),
    waitForURL: async (pattern: string) => {
      calls.waitForURLs.push(pattern);
    },
    waitForFunction: async (fn: (arg: never) => unknown, arg: unknown) => {
      calls.waitForFunctions.push({ fn, arg });
    },
    route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
      calls.routes.push({ matcher, handler });
    },
    addInitScript: async (fn: unknown) => {
      calls.initScripts.push(fn);
    },
    evaluate: async (fn: (arg: never) => unknown, arg: unknown) => {
      calls.evaluates.push({ fn, arg });
      return config.evaluateImpl?.(fn, arg);
    },
  };
  return { page: page as unknown as Page, calls };
}

function fakeRoute(method: string, url: string): { route: Route; continued: () => boolean } {
  let continued = false;
  const route = {
    request: () => ({ method: () => method, url: () => url }),
    continue: async () => {
      continued = true;
    },
  };
  return { route: route as unknown as Route, continued: () => continued };
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("archives tab opens", () => {
  it("opens the work-items list and settles on the strip", async () => {
    const { page, calls } = stubPage({ url: "http://new.test/acme/projects/p1/archives/issues" });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenIssuesList("acme", "p1");

    expect(calls.goto).toEqual(["/acme/projects/p1/archives/issues"]);
    expect(calls.reloads).toBe(0);
  });

  it("reloads a stalled open once before settling", async () => {
    const { page, calls } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-tab-strip"]'
          ? loc({ count: async () => (calls.reloads > 0 ? 1 : 0) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenIssuesList("acme", "p1");

    expect(calls.reloads).toBe(1);
  });

  it("throws when the strip never settles", async () => {
    const { page, calls } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-tab-strip"]' ? loc({ count: async () => 0 }) : loc(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesTabOpen("acme", "p1", "cycles")).rejects.toThrow("archives strip never settled");
    expect(calls.goto).toEqual(["/acme/projects/p1/archives/cycles"]);
    expect(calls.reloads).toBe(1);
  });

  it("opens a tab directly by address", async () => {
    const { page, calls } = stubPage({ url: "http://new.test/acme/projects/p1/archives/modules" });
    const driver = new WebNewDriver(page);

    await driver.archivesTabOpen("acme", "p1", "modules");

    expect(calls.goto).toEqual(["/acme/projects/p1/archives/modules"]);
  });

  it("opens the list past a flashing skeleton", async () => {
    let looks = 0;
    const { page } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => (++looks <= 2 ? 1 : 0) }) });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenList("acme", "p1");

    expect(looks).toBeGreaterThan(2);
  });

  it("returns from the list open even when the loader sticks", async () => {
    const { page, calls } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => 1, isVisible: async () => true }) });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenList("acme", "p1");

    expect(calls.timeouts.length).toBeGreaterThan(0);
  });

  it("reloads the cycles tab and settles on the strip", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesReload();

    expect(calls.reloads).toBe(1);
  });
});

describe("archives tab strip reads", () => {
  it("clicks through the strip and waits for the address", async () => {
    const clicked: string[] = [];
    const { page, calls } = stubPage({
      locatorFor: (selector) => loc({ click: async () => void clicked.push(selector) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenTab("cycles");

    expect(clicked).toEqual(['[data-testid="archives-tab-strip"] a[href*="/archives/cycles"]']);
    expect(calls.waitForURLs).toEqual(["**/archives/cycles**"]);
  });

  it("clicks a tab and lands on its key", async () => {
    let reads = 0;
    const { page } = stubPage({
      url: () =>
        ++reads <= 1
          ? "http://new.test/acme/projects/p1/archives/issues"
          : "http://new.test/acme/projects/p1/archives/modules",
    });
    const driver = new WebNewDriver(page);

    await driver.archivesTabClick("modules");

    expect(reads).toBeGreaterThan(1);
  });

  it("throws when the tab click never lands", async () => {
    const { page } = stubPage({ url: "http://new.test/acme/projects/p1/archives/issues" });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesTabClick("cycles")).rejects.toThrow("never landed on cycles");
  });

  it("names the rendered tabs in strip order", async () => {
    const { page } = stubPage({
      locatorFor: (selector) => loc({ count: async () => (selector.includes("/archives/cycles") ? 0 : 1) }),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesTabNames()).resolves.toEqual(["Work items", "Modules"]);
  });

  it("reads the active tab from the address", async () => {
    for (const [address, label] of [
      ["http://new.test/acme/projects/p1/archives/issues", "Work items"],
      ["http://new.test/acme/projects/p1/archives/cycles", "Cycles"],
      ["http://new.test/acme/projects/p1/archives/modules", "Modules"],
      ["http://new.test/acme/projects/p1/archives/issues/abc-123", "Work items"],
    ] as const) {
      const { page } = stubPage({ url: address });
      const driver = new WebNewDriver(page);
      await expect(driver.archivesActiveTab()).resolves.toBe(label);
    }
  });

  it("throws the active tab off the archives tree", async () => {
    const { page } = stubPage({ url: "http://new.test/acme/projects/p1/issues" });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesActiveTab()).rejects.toThrow("on no archives tab");
  });

  it("reads the active tab key from the address", async () => {
    for (const [address, key] of [
      ["http://new.test/acme/projects/p1/archives/issues", "issues"],
      ["http://new.test/acme/projects/p1/archives/cycles?peekCycle=x", "cycles"],
      ["http://new.test/acme/projects/p1/archives/modules/", "modules"],
    ] as const) {
      const { page } = stubPage({ url: address });
      const driver = new WebNewDriver(page);
      await expect(driver.archivesActiveTabKey()).resolves.toBe(key);
    }
  });

  it("throws the active key off the archives tree", async () => {
    const { page } = stubPage({ url: "http://new.test/acme/projects" });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesActiveTabKey()).rejects.toThrow("no archives tab is active");
  });
});

describe("archives header reads", () => {
  it("reports the back control by presence", async () => {
    const present = stubPage({
      locatorFor: (selector) => (selector === '[data-testid="archives-back"]' ? loc({ count: async () => 1 }) : loc()),
    });
    await expect(new WebNewDriver(present.page).archivesBackPresent()).resolves.toBe(true);

    const absent = stubPage({
      locatorFor: (selector) => (selector === '[data-testid="archives-back"]' ? loc({ count: async () => 0 }) : loc()),
    });
    await expect(new WebNewDriver(absent.page).archivesBackPresent()).resolves.toBe(false);
  });

  it("clicks the back control", async () => {
    let clicked = 0;
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-back"]' ? loc({ click: async () => void (clicked += 1) }) : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesClickBack();

    expect(clicked).toBe(1);
  });

  it("reads the count badge", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]' ? loc({ innerText: async () => "  2 " }) : loc(),
    });
    await expect(new WebNewDriver(page).archivesCountBadge()).resolves.toBe("2");
  });

  it("reads no badge without the element, blank text, or a torn read", async () => {
    const absent = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]' ? loc({ count: async () => 0 }) : loc(),
    });
    await expect(new WebNewDriver(absent.page).archivesCountBadge()).resolves.toBeNull();

    const blank = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]' ? loc({ innerText: async () => "  " }) : loc(),
    });
    await expect(new WebNewDriver(blank.page).archivesCountBadge()).resolves.toBeNull();

    const torn = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]'
          ? loc({
              innerText: async () => {
                throw new Error("detached");
              },
            })
          : loc(),
    });
    await expect(new WebNewDriver(torn.page).archivesCountBadge()).resolves.toBeNull();
  });

  it("hovers the badge and reads its tip", async () => {
    let hovered = 0;
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]' ? loc({ hover: async () => void (hovered += 1) }) : loc(),
      roleFor: (role) => (role === "tooltip" ? loc({ innerText: async () => " 2 archived work items " }) : loc()),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesCountBadgeTooltip()).resolves.toBe("2 archived work items");
    expect(hovered).toBe(1);
  });

  it("reads no tip without the badge, a missing popup, or blank text", async () => {
    const absent = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-count-badge"]' ? loc({ count: async () => 0 }) : loc(),
    });
    await expect(new WebNewDriver(absent.page).archivesCountBadgeTooltip()).resolves.toBeNull();

    const missing = stubPage({
      roleFor: (role) =>
        role === "tooltip"
          ? loc({
              waitFor: async () => {
                throw new Error("never opened");
              },
            })
          : loc(),
    });
    await expect(new WebNewDriver(missing.page).archivesCountBadgeTooltip()).resolves.toBeNull();

    const blank = stubPage({
      roleFor: (role) => (role === "tooltip" ? loc({ innerText: async () => "" }) : loc()),
    });
    await expect(new WebNewDriver(blank.page).archivesCountBadgeTooltip()).resolves.toBeNull();
  });

  it("reads the page title", async () => {
    const { page } = stubPage({ title: "Web - Archived work items" });
    await expect(new WebNewDriver(page).archivesPageTitle()).resolves.toBe("Web - Archived work items");
  });

  it("reads page text by visibility", async () => {
    const shown = stubPage({ textFor: () => loc({ count: async () => 1, isVisible: async () => true }) });
    await expect(new WebNewDriver(shown.page).archivesPageTextPresent("Cycle one")).resolves.toBe(true);

    const hidden = stubPage({ textFor: () => loc({ count: async () => 0 }) });
    await expect(new WebNewDriver(hidden.page).archivesPageTextPresent("Cycle one")).resolves.toBe(false);
  });
});

describe("archives direct load", () => {
  it("reports a settled direct load", async () => {
    const { page, calls } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => 0 }) });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesDirectLoadRenders("acme", "p1")).resolves.toBe(true);
    expect(calls.goto).toEqual(["/acme/projects/p1/archives/modules"]);
  });

  it("reports a missing shell as not rendered", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-tab-strip"]'
          ? loc({
              waitFor: async () => {
                throw new Error("never painted");
              },
            })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesDirectLoadRenders("acme", "p1")).resolves.toBe(false);
  });

  it("reports a stuck loader as not rendered", async () => {
    const { page } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => 1, isVisible: async () => true }) });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesDirectLoadRenders("acme", "p1")).resolves.toBe(false);
  });
});

describe("archives skeleton probes", () => {
  it("scopes the skeleton read to the content slot", async () => {
    const shown = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => 1, isVisible: async () => true }) });
        }
        return loc({ count: async () => 0 });
      },
    });
    await expect(new WebNewDriver(shown.page).archivesSkeletonVisible()).resolves.toBe(true);

    const settled = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({ nested: () => loc({ count: async () => 0 }) });
        }
        return loc({ count: async () => 1, isVisible: async () => true });
      },
    });
    // A page-wide shimmer outside the slot does not count once the shell mounts.
    await expect(new WebNewDriver(settled.page).archivesSkeletonVisible()).resolves.toBe(false);
  });

  it("falls back to a page-wide read before the shell mounts", async () => {
    const shown = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ count: async () => 0 })
          : loc({ count: async () => 1, isVisible: async () => true }),
    });
    await expect(new WebNewDriver(shown.page).archivesSkeletonVisible()).resolves.toBe(true);

    const absent = stubPage({ locatorFor: () => loc({ count: async () => 0 }) });
    await expect(new WebNewDriver(absent.page).archivesSkeletonVisible()).resolves.toBe(false);
  });

  it("arms the skeleton observer as an init script", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesArmSkeletonObserver();

    expect(calls.initScripts).toEqual([archivesSkeletonObserverBoot]);
  });

  it("records a shimmer mount the observer sees", async () => {
    archivesSkeletonObserverBoot();
    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(false);

    const shimmer = document.createElement("div");
    shimmer.className = "animate-shimmer";
    document.body.appendChild(shimmer);
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(true);
  });

  it("records a pulse applied through a class change", async () => {
    archivesSkeletonObserverBoot();
    const block = document.createElement("div");
    document.body.appendChild(block);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(false);

    block.setAttribute("class", "animate-pulse");
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(true);
  });

  it("records a nested skeleton testid wrapper", async () => {
    archivesSkeletonObserverBoot();
    const wrapper = document.createElement("div");
    const inner = document.createElement("div");
    inner.setAttribute("data-testid", "cycles-skeleton");
    wrapper.appendChild(inner);
    document.body.appendChild(wrapper);
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(true);
  });

  it("ignores plain content and plain class changes", async () => {
    archivesSkeletonObserverBoot();
    document.body.appendChild(document.createTextNode("rows"));
    const block = document.createElement("div");
    document.body.appendChild(block);
    block.setAttribute("class", "plain");
    await new Promise((resolve) => setTimeout(resolve, 10));

    expect((window as unknown as Record<string, unknown>)["__paritySkeletonSeen"]).toBe(false);
  });

  it("reads whether the observer saw a skeleton", async () => {
    const seen = stubPage({ evaluateImpl: () => true });
    await expect(new WebNewDriver(seen.page).archivesSkeletonWasSeen()).resolves.toBe(true);

    const unseen = stubPage({ evaluateImpl: () => false });
    await expect(new WebNewDriver(unseen.page).archivesSkeletonWasSeen()).resolves.toBe(false);

    const torn = stubPage({
      evaluateImpl: () => {
        throw new Error("navigated away");
      },
    });
    await expect(new WebNewDriver(torn.page).archivesSkeletonWasSeen()).resolves.toBe(false);
  });
});

describe("archives traffic shaping", () => {
  it("delays only archived GETs, twelve at most", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);
    const timeouts = vi.spyOn(globalThis, "setTimeout");

    await driver.archivesDelayNextListReads(25);

    expect(calls.routes).toHaveLength(1);
    const installed = calls.routes[0];
    if (!installed) throw new Error("delay route not installed");
    const matches = installed.matcher as (url: URL) => boolean;
    expect(matches(new URL("http://api.test/api/workspaces/acme/projects/p1/archived-issues/"))).toBe(true);
    expect(matches(new URL("http://api.test/api/workspaces/acme/projects/p1/archived-cycles/"))).toBe(true);
    expect(matches(new URL("http://api.test/api/workspaces/acme/projects/p1/issues/x/archive/"))).toBe(true);
    expect(matches(new URL("http://api.test/api/workspaces/acme/projects/"))).toBe(false);

    for (let index = 0; index < 13; index += 1) {
      const { route, continued } = fakeRoute("GET", "http://api.test/api/workspaces/acme/projects/p1/archived-issues/");
      await installed.handler(route);
      expect(continued()).toBe(true);
    }
    const { route: post, continued: postContinued } = fakeRoute(
      "POST",
      "http://api.test/api/workspaces/acme/projects/p1/issues/x/archive/"
    );
    await installed.handler(post);
    expect(postContinued()).toBe(true);
    expect(timeouts).toHaveBeenCalledTimes(12);
  });

  it("counts archived reads and archive writes per family", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesBeginTrafficSpy();

    expect(calls.routes).toHaveLength(1);
    const installed = calls.routes[0];
    if (!installed) throw new Error("traffic spy not installed");
    expect(installed.matcher).toBe("**/api/**");
    const requests: Array<[string, string]> = [
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/archived-issues/?cursor=a"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/archived-cycles/"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/archived-modules/"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/issues/x/archive/"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/archived-cycles/y/"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/archived-modules/z/"],
      ["POST", "http://api.test/api/workspaces/acme/projects/p1/cycles/y/archive/"],
      ["DELETE", "http://api.test/api/workspaces/acme/projects/p1/modules/z/archive/"],
      ["GET", "http://api.test/api/workspaces/acme/projects/p1/issues/"],
      ["PUT", "http://api.test/api/workspaces/acme/projects/p1/cycles/y/archive/"],
      ["POST", "http://api.test/api/workspaces/acme/projects/p1/issues/"],
    ];
    for (const [method, url] of requests) {
      const { route, continued } = fakeRoute(method, url);
      await installed.handler(route);
      expect(continued()).toBe(true);
    }

    await expect(driver.archivesTrafficCounts()).resolves.toEqual({
      issuesReads: 1,
      cyclesReads: 1,
      modulesReads: 1,
      detailReads: 3,
      writes: 2,
    });

    // Beginning again resets the counters.
    await driver.archivesBeginTrafficSpy();
    await expect(driver.archivesTrafficCounts()).resolves.toEqual({
      issuesReads: 0,
      cyclesReads: 0,
      modulesReads: 0,
      detailReads: 0,
      writes: 0,
    });
  });

  it("returns a copy of the traffic counters", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);
    await driver.archivesBeginTrafficSpy();
    const installed = calls.routes[0];
    if (!installed) throw new Error("traffic spy not installed");
    const { route } = fakeRoute("GET", "http://api.test/api/workspaces/acme/projects/p1/archived-issues/");
    await installed.handler(route);

    const counts = await driver.archivesTrafficCounts();
    counts.issuesReads = 99;

    await expect(driver.archivesTrafficCounts()).resolves.toMatchObject({ issuesReads: 1 });
  });
});

describe("archives navigation entries", () => {
  it("navigates client-side without reloading", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesClientNavigate("/acme/projects/p1/archives/modules");

    expect(calls.reloads).toBe(0);
    expect(calls.evaluates).toHaveLength(1);
    const evaluated = calls.evaluates[0];
    if (!evaluated) throw new Error("client navigate never evaluated");
    expect(evaluated.arg).toBe("/acme/projects/p1/archives/modules");
    let popped = 0;
    window.addEventListener("popstate", () => {
      popped += 1;
    });
    evaluated.fn("/acme/projects/p1/archives/modules" as never);
    expect(window.location.pathname).toBe("/acme/projects/p1/archives/modules");
    expect(popped).toBe(1);

    expect(calls.waitForFunctions).toHaveLength(1);
    const waited = calls.waitForFunctions[0];
    if (!waited) throw new Error("client navigate never waited");
    expect(waited.fn("/acme/projects/p1/archives/modules" as never)).toBe(true);
  });

  it("follows the sidebar project menu into the archives", async () => {
    const seen: string[] = [];
    const { page, calls } = stubPage({
      locatorFor: (selector) => {
        seen.push(selector);
        if (selector.startsWith("li[data-project-row")) {
          return loc({
            byTestId: (id) => {
              seen.push(`testid:${id}`);
              return loc();
            },
          });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesGoToProjectArchives("p1");

    expect(seen).toContain('li[data-project-row="p1"]');
    expect(seen).toContain("testid:project-row-menu");
    expect(calls.waitForFunctions).toHaveLength(1);
    const waited = calls.waitForFunctions[0];
    if (!waited) throw new Error("project archives entry never waited");
    window.history.pushState({}, "", "/acme/projects/p1/archives/issues");
    expect(waited.fn("p1" as never)).toBe(true);
  });

  it("enters the archives when the row hover refuses", async () => {
    const { page, calls } = stubPage({
      locatorFor: (selector) => {
        if (selector.startsWith("li[data-project-row")) {
          return loc({
            hover: async () => {
              throw new Error("covered");
            },
          });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesGoToProjectArchives("p1");

    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("selects a strip tab by label", async () => {
    for (const [tab, slug] of [
      ["Modules", "modules"],
      ["Cycles", "cycles"],
    ] as const) {
      const clicked: string[] = [];
      const { page, calls } = stubPage({
        locatorFor: (selector) => loc({ click: async () => void clicked.push(selector) }),
      });
      const driver = new WebNewDriver(page);

      await driver.archivesSelectArchivesTab(tab);

      expect(clicked).toEqual([`[data-testid="archives-tab-strip"] a[href*="/archives/${slug}"]`]);
      expect(calls.waitForFunctions).toHaveLength(1);
      const waited = calls.waitForFunctions[0];
      if (!waited) throw new Error("tab select never waited");
      window.history.pushState({}, "", `/acme/projects/p1/archives/${slug}`);
      expect(waited.fn(slug as never)).toBe(true);
    }
  });

  it("primes on the live cycles page and enters through the sidebar", async () => {
    let looks = 0;
    const { page, calls } = stubPage({
      textFor: () => loc({ count: async () => (++looks <= 2 ? 0 : 1) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPrimeAndEnter("acme", "p1", "Cycle one");

    expect(calls.goto).toEqual(["/acme/projects/p1/cycles/"]);
    expect(looks).toBeGreaterThan(2);
    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("throws when the live cycle never renders", async () => {
    const { page } = stubPage({ textFor: () => loc({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesPrimeAndEnter("acme", "p1", "Cycle one")).rejects.toThrow("live cycle never rendered");
  });
});
