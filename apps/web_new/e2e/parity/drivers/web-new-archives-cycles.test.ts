// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archived-cycles driver family (NEWFRONT-701,
// ARCH-014–019). A stub Page replays settled tabs, slow fetches,
// peeks, search boxes, filter menus and dialogs here instead of a
// browser; the browser-serialized predicates are captured and invoked
// against jsdom so every shipped line runs.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

function loc(
  overrides: {
    count?: () => Promise<number>;
    innerText?: () => Promise<string>;
    inputValue?: () => Promise<string>;
    getAttribute?: (name: string) => Promise<string | null>;
    isVisible?: () => Promise<boolean>;
    waitFor?: (options?: unknown) => Promise<void>;
    click?: (options?: unknown) => Promise<void>;
    hover?: (options?: unknown) => Promise<void>;
    fill?: (text: string, options?: unknown) => Promise<void>;
    press?: (key: string, options?: unknown) => Promise<void>;
    evaluate?: (fn: (element: never) => unknown) => unknown;
    scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
    nth?: (index: number) => Locator;
    last?: () => Locator;
    nested?: (selector: string) => Locator;
    byTestId?: (id: string) => Locator;
    byRole?: (role: string, options?: unknown) => Locator;
    filtered?: (options?: unknown) => Locator;
  } = {}
): Locator {
  const node: Record<string, unknown> = {};
  node["count"] = overrides.count ?? (async () => 1);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["inputValue"] = overrides.inputValue ?? (async () => "");
  node["getAttribute"] = overrides.getAttribute ?? (async () => null);
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["hover"] = overrides.hover ?? (async () => undefined);
  node["fill"] = overrides.fill ?? (async () => undefined);
  node["press"] = overrides.press ?? (async () => undefined);
  node["evaluate"] = (fn: (element: never) => unknown) => {
    try {
      const run = overrides.evaluate ?? ((target: (element: never) => unknown) => target({} as never));
      return Promise.resolve(run(fn));
    } catch (error) {
      return Promise.reject(error);
    }
  };
  node["scrollIntoViewIfNeeded"] = overrides.scrollIntoViewIfNeeded ?? (async () => undefined);
  const self = node as unknown as Locator;
  node["first"] = () => self;
  node["nth"] = (index: number) => overrides.nth?.(index) ?? self;
  node["last"] = () => overrides.last?.() ?? self;
  node["locator"] = (selector: string) => overrides.nested?.(selector) ?? self;
  node["getByTestId"] = (id: string) => overrides.byTestId?.(id) ?? self;
  node["getByRole"] = (role: string, options?: unknown) => overrides.byRole?.(role, options) ?? self;
  node["filter"] = (options?: unknown) => overrides.filtered?.(options) ?? self;
  return self;
}

interface RouteCall {
  matcher: unknown;
  handler: (route: Route) => Promise<void>;
}

interface PageCalls {
  goto: string[];
  reloads: number;
  waitForURLs: string[];
  waitForFunctions: Array<{ fn: (arg: never) => unknown; arg: unknown }>;
  waitForResponses: Array<{ predicate: (response: never) => unknown; options: unknown }>;
  routes: RouteCall[];
  unroutes: unknown[];
  initScripts: unknown[];
  keyPresses: string[];
}

function stubPage(config: {
  url?: string | (() => string);
  locatorFor?: (selector: string) => Locator;
  roleFor?: (role: string, options?: unknown) => Locator;
  testIdFor?: (id: string) => Locator;
  waitForFunctionImpl?: (fn: (arg: never) => unknown, arg: unknown) => unknown;
  waitForResponseImpl?: (predicate: (response: never) => unknown) => unknown;
  evaluateImpl?: () => unknown;
}): { page: Page; calls: PageCalls } {
  const calls: PageCalls = {
    goto: [],
    reloads: 0,
    waitForURLs: [],
    waitForFunctions: [],
    waitForResponses: [],
    routes: [],
    unroutes: [],
    initScripts: [],
    keyPresses: [],
  };
  const page = {
    goto: async (url: string) => {
      calls.goto.push(url);
    },
    reload: async () => {
      calls.reloads += 1;
    },
    waitForLoadState: async () => undefined,
    url: () => (typeof config.url === "function" ? config.url() : (config.url ?? "http://new.test/")),
    locator: (selector: string) => config.locatorFor?.(selector) ?? loc(),
    getByRole: (role: string, options?: unknown) => config.roleFor?.(role, options) ?? loc(),
    getByTestId: (id: string) => config.testIdFor?.(id) ?? loc(),
    getByText: () => loc(),
    waitForURL: async (pattern: string) => {
      calls.waitForURLs.push(pattern);
    },
    waitForFunction: async (fn: (arg: never) => unknown, arg: unknown) => {
      calls.waitForFunctions.push({ fn, arg });
      return config.waitForFunctionImpl?.(fn, arg);
    },
    waitForResponse: async (predicate: (response: never) => unknown, options: unknown) => {
      calls.waitForResponses.push({ predicate, options });
      return config.waitForResponseImpl?.(predicate) ?? { stub: true };
    },
    route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
      calls.routes.push({ matcher, handler });
    },
    unroute: async (matcher: unknown) => {
      calls.unroutes.push(matcher);
    },
    addInitScript: async (fn: unknown) => {
      calls.initScripts.push(fn);
    },
    evaluate: async () => config.evaluateImpl?.(),
    keyboard: {
      press: async (key: string) => {
        calls.keyPresses.push(key);
      },
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

function fakeResponse(method: string, url: string): never {
  return { request: () => ({ method: () => method }), url: () => url } as never;
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("archived-cycles tab opens", () => {
  it("enters through the live screen plus client-side navigation", async () => {
    const { page, calls } = stubPage({ url: "http://new.test/acme/projects/p1/cycles" });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesOpenTabViaLive("acme", "p1");

    expect(calls.goto).toEqual(["/acme/projects/p1/cycles"]);
    expect(calls.waitForURLs).toEqual(["**/archives/cycles**"]);
    expect(calls.waitForFunctions.length).toBeGreaterThan(0);
  });

  it("opens the tab raw without settling", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesOpenTabRaw("acme", "p1");

    expect(calls.goto).toEqual(["/acme/projects/p1/archives/cycles"]);
    expect(calls.waitForFunctions).toEqual([]);
  });

  it("opens the live screen settled", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesOpenLive("acme", "p1");

    expect(calls.goto).toEqual(["/acme/projects/p1/cycles"]);
  });

  it("opens the live screen through the cycle-peek alias too", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesOpenLiveCycles("acme", "p1");

    expect(calls.goto).toEqual(["/acme/projects/p1/cycles"]);
  });

  it("opens the tab from the live screen through the sidebar entry", async () => {
    const { page, calls } = stubPage({ url: "http://new.test/acme/projects/p1/cycles" });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenCyclesTab("acme", "p1");

    expect(calls.goto).toEqual([]);
    expect(calls.waitForURLs).toEqual(["**/archives/cycles**"]);
  });

  it("reads the skeleton while it shows and after it clears", async () => {
    const shownPage = stubPage({ testIdFor: (id) => (id === "archived-cycles-skeleton" ? loc() : loc()) });
    expect(await new WebNewDriver(shownPage.page).archivesCyclesSkeletonVisible()).toBe(true);

    const hiddenPage = stubPage({
      testIdFor: (id) => (id === "archived-cycles-skeleton" ? loc({ count: async () => 0 }) : loc()),
    });
    expect(await new WebNewDriver(hiddenPage.page).archivesCyclesSkeletonVisible()).toBe(false);
  });
});

describe("archived-cycles rows and groups", () => {
  it("reads row names from the menu triggers in order", async () => {
    const labels = ["Cycle actions for Archived alpha", "Cycle actions for Archived beta", null];
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === 'div[data-testid="cycle-row"] button[data-testid="cycle-row-menu-trigger"]'
          ? loc({
              count: async () => labels.length,
              nth: (index) =>
                loc({
                  getAttribute: async () => labels[index] ?? null,
                }),
            })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesVisibleNames()).toEqual(["Archived alpha", "Archived beta"]);
    expect(await driver.archivesCycleRowNames()).toEqual(["Archived alpha", "Archived beta"]);
  });

  it("reads no names when no rows render", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === 'div[data-testid="cycle-row"] button[data-testid="cycle-row-menu-trigger"]'
          ? loc({ count: async () => 0 })
          : loc(),
    });

    expect(await new WebNewDriver(page).archivesCyclesVisibleNames()).toEqual([]);
  });

  it("reads group headings on the live screen", async () => {
    const labels: Array<string | null> = ["Upcoming cycle", "Completed cycle", null, ""];
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === 'section[data-testid^="cycle-group-"]'
          ? loc({
              count: async () => labels.length,
              nth: (index) =>
                loc({
                  getAttribute: async () => labels[index] ?? null,
                }),
            })
          : loc(),
    });

    expect(await new WebNewDriver(page).archivesCyclesGroupHeadings()).toEqual(["Upcoming cycle", "Completed cycle"]);
  });

  it("reads no headings on the archived tab", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === 'section[data-testid^="cycle-group-"]' ? loc({ count: async () => 0 }) : loc(),
    });

    expect(await new WebNewDriver(page).archivesCyclesGroupHeadings()).toEqual([]);
  });

  it("leaves an expanded live group alone", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      locatorFor: () =>
        loc({
          byRole: () =>
            loc({
              getAttribute: async () => "true",
              click: async () => void clicks.push("toggle"),
            }),
        }),
    });

    await new WebNewDriver(page).archivesCyclesLiveGroupOpen("Completed");

    expect(clicks).toEqual([]);
    expect(calls.waitForFunctions).toEqual([]);
  });

  it("expands a collapsed live group and waits for it", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      locatorFor: () =>
        loc({
          byRole: () =>
            loc({
              getAttribute: async () => "false",
              click: async () => void clicks.push("toggle"),
            }),
        }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesLiveGroupOpen("Completed");

    expect(clicks).toEqual(["toggle"]);
    expect(calls.waitForFunctions).toHaveLength(1);
    expect(calls.waitForFunctions[0]?.arg).toBe("Completed");
    document.body.innerHTML = `<section data-testid="cycle-group-completed" aria-label="Completed cycle"><button aria-expanded="true"></button></section>`;
    const seen = await calls.waitForFunctions[0]?.fn("Completed" as never);
    expect(seen).toBe(true);
    document.body.innerHTML = `<section data-testid="cycle-group-completed" aria-label="Completed cycle"><button aria-expanded="false"></button></section>`;
    expect(await calls.waitForFunctions[0]?.fn("Completed" as never)).toBe(false);
  });
});

describe("archived-cycle peek", () => {
  it("opens the peek through the row's quick look and waits for the param", async () => {
    const clicks: string[] = [];
    const hovers: string[] = [];
    const { page, calls } = stubPage({
      locatorFor: () =>
        loc({
          hover: async () => void hovers.push("row"),
          byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }),
        }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesOpenPeek("Archived alpha");

    expect(hovers).toEqual(["row"]);
    expect(clicks).toEqual([JSON.stringify({ name: "Quick look at Archived alpha" })]);
    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("opens the peek through the cycle-peek alias too", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesOpenCyclePeek("Archived alpha");

    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("opens a shared peek link without settling", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesOpenPeekLink("acme", "p1", "c1");

    expect(calls.goto).toEqual(["/acme/projects/p1/archives/cycles?peekCycle=c1"]);
    expect(calls.waitForFunctions).toEqual([]);
  });

  it("reads the peek heading on both peek-name readers", async () => {
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycle-preview"
          ? loc({ byRole: () => loc({ innerText: async () => "  Archived alpha " }) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesPeekName()).toBe("Archived alpha");
    expect(await driver.archivesCyclePeekName()).toBe("Archived alpha");
  });

  it("reads no peek name when no peek is open", async () => {
    const { page } = stubPage({
      testIdFor: (id) => (id === "archived-cycle-preview" ? loc({ count: async () => 0 }) : loc()),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesPeekName()).toBe(null);
    expect(await driver.archivesCyclePeekName()).toBe(null);
  });

  it("reads the peek param from the address", async () => {
    const withParam = stubPage({ url: "http://new.test/acme/projects/p1/archives/cycles?peekCycle=c1" });
    expect(await new WebNewDriver(withParam.page).archivesCyclesPeekParam()).toBe("c1");

    const withoutParam = stubPage({ url: "http://new.test/acme/projects/p1/archives/cycles" });
    expect(await new WebNewDriver(withoutParam.page).archivesCyclesPeekParam()).toBe(null);
  });

  it("closes the peek and waits for the param to clear", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycle-preview"
          ? loc({ byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesClosePeek();

    expect(clicks).toEqual([JSON.stringify({ name: "Close preview" })]);
    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("closes the peek through the cycle-peek alias too", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesCloseCyclePeek();

    expect(calls.waitForFunctions).toHaveLength(1);
  });

  it("proves a locked read-only panel with its summary", async () => {
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id === "archived-cycle-preview") {
          return loc({
            byRole: (role, _options) => {
              if (role === "heading") return loc({ innerText: async () => "Readonly Cycle" });
              return loc({ count: async () => 0 });
            },
            byTestId: () => loc(),
          });
        }
        return loc();
      },
    });

    expect(await new WebNewDriver(page).archivesCyclePeekReadOnly()).toEqual({
      name: "Readonly Cycle",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: true,
    });
  });

  it("reports an unlocked panel without a summary", async () => {
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id === "archived-cycle-preview") {
          return loc({
            count: async () => 1,
            byRole: (role, _options) => {
              if (role === "heading") return loc({ innerText: async () => "Readonly Cycle" });
              return loc({ count: async () => 1 });
            },
            byTestId: () => loc({ count: async () => 0 }),
          });
        }
        return loc();
      },
    });

    expect(await new WebNewDriver(page).archivesCyclePeekReadOnly()).toEqual({
      name: "Readonly Cycle",
      statusLocked: false,
      addLinkOffered: true,
      summaryShown: false,
    });
  });

  it("reads an absent panel as unnamed, locked, unoffered and summary-less", async () => {
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycle-preview"
          ? loc({
              count: async () => 0,
              byRole: () => loc({ count: async () => 0 }),
              byTestId: () => loc({ count: async () => 0 }),
            })
          : loc(),
    });

    expect(await new WebNewDriver(page).archivesCyclePeekReadOnly()).toEqual({
      name: null,
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: false,
    });
  });
});

describe("archived-cycles search", () => {
  it("reads whether the box is expanded", async () => {
    const open = stubPage({});
    expect(await new WebNewDriver(open.page).archivesCyclesSearchExpanded()).toBe(true);

    const shut = stubPage({
      testIdFor: (id) => (id === "archived-cycles-search-box" ? loc({ count: async () => 0 }) : loc()),
    });
    expect(await new WebNewDriver(shut.page).archivesCyclesSearchExpanded()).toBe(false);
  });

  it("opens the box and waits for focus", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycles-search-toggle" ? loc({ click: async () => void clicks.push(id) }) : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesSearchOpen();

    expect(clicks).toEqual(["archived-cycles-search-toggle"]);
    expect(calls.waitForFunctions).toHaveLength(1);
    document.body.innerHTML = `<input aria-label="Search archived cycles" />`;
    const input = document.querySelector("input");
    input?.focus();
    expect(await calls.waitForFunctions[0]?.fn(undefined as never)).toBe(true);
    input?.blur();
    expect(await calls.waitForFunctions[0]?.fn(undefined as never)).toBe(false);
  });

  it("reads focus from the input element", async () => {
    const focused = stubPage({
      testIdFor: (id) => (id === "archived-cycles-search-input" ? loc({ evaluate: () => true }) : loc()),
    });
    expect(await new WebNewDriver(focused.page).archivesCyclesSearchFocused()).toBe(true);

    const blurred = stubPage({
      testIdFor: (id) => (id === "archived-cycles-search-input" ? loc({ evaluate: () => false }) : loc()),
    });
    expect(await new WebNewDriver(blurred.page).archivesCyclesSearchFocused()).toBe(false);

    const detached = stubPage({
      testIdFor: (id) =>
        id === "archived-cycles-search-input"
          ? loc({
              evaluate: () => {
                throw new Error("detached");
              },
            })
          : loc(),
    });
    expect(await new WebNewDriver(detached.page).archivesCyclesSearchFocused()).toBe(false);

    const collapsed = stubPage({
      testIdFor: (id) => (id === "archived-cycles-search-input" ? loc({ count: async () => 0 }) : loc()),
    });
    expect(await new WebNewDriver(collapsed.page).archivesCyclesSearchFocused()).toBe(false);
  });

  it("fills and reads the box text", async () => {
    const fills: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycles-search-input"
          ? loc({ fill: async (text) => void fills.push(text), inputValue: async () => "alpha" })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesSearchFill("alpha");
    expect(fills).toEqual(["alpha"]);
    expect(await driver.archivesCyclesSearchText()).toBe("alpha");
  });

  it("reads empty text when the box is collapsed", async () => {
    const { page } = stubPage({
      testIdFor: (id) => (id === "archived-cycles-search-input" ? loc({ count: async () => 0 }) : loc()),
    });

    expect(await new WebNewDriver(page).archivesCyclesSearchText()).toBe("");
  });

  it("sends Escape to the input", async () => {
    const presses: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycles-search-input" ? loc({ press: async (key) => void presses.push(key) }) : loc(),
    });

    await new WebNewDriver(page).archivesCyclesSearchEscape();

    expect(presses).toEqual(["Escape"]);
  });

  it("clears through the in-box control and waits for the collapse", async () => {
    const clicks: string[] = [];
    const waits: unknown[] = [];
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id === "archived-cycles-search-clear") return loc({ click: async () => void clicks.push(id) });
        if (id === "archived-cycles-search-box") return loc({ waitFor: async (options) => void waits.push(options) });
        return loc();
      },
    });

    await new WebNewDriver(page).archivesCyclesSearchClear();

    expect(clicks).toEqual(["archived-cycles-search-clear"]);
    expect(waits).toEqual([{ state: "detached", timeout: 30_000 }]);
  });

  it("clicks away through the header spacer", async () => {
    const clicks: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "archived-cycles-header-spacer" ? loc({ click: async () => void clicks.push(id) }) : loc(),
    });

    await new WebNewDriver(page).archivesCyclesClickAway();

    expect(clicks).toEqual(["archived-cycles-header-spacer"]);
  });
});

describe("archived-cycles filters", () => {
  it("opens and closes the shared filter menu", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      testIdFor: (id) => (id === "cycles-filters-trigger" ? loc({ click: async () => void clicks.push(id) }) : loc()),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesFiltersOpen();
    await driver.archivesCyclesFiltersClose();

    expect(clicks).toEqual(["cycles-filters-trigger"]);
    expect(calls.keyPresses).toEqual(["Escape"]);
  });

  it("reads sections and option names from the shared panel", async () => {
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id !== "cycles-filters-panel") return loc();
        return loc({
          nested: (selector) => {
            if (selector === "[data-section-title]") {
              const titles = ["Start date", "Due date"];
              return loc({
                count: async () => titles.length,
                nth: (index) => loc({ getAttribute: async () => titles[index] ?? null }),
              });
            }
            const names = ["1 week from now", "Custom"];
            return loc({
              count: async () => names.length,
              nth: (index) => loc({ getAttribute: async () => names[index] ?? null }),
            });
          },
        });
      },
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesFilterSections()).toEqual(["Start date", "Due date"]);
    expect(await driver.archivesCyclesFilterOptionNames()).toContain("1 week from now");
  });

  it("picks an option within its section", async () => {
    const picks: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id !== "cycles-filters-panel") return loc();
        return loc({
          nested: (selector) =>
            loc({
              byRole: (_role, options) =>
                loc({ click: async () => void picks.push(`${selector} :: ${JSON.stringify(options)}`) }),
            }),
        });
      },
    });

    await new WebNewDriver(page).archivesCyclesFilterPick("Start date", "1 week from now");

    expect(picks).toEqual([`[data-section-title*="Start date"] :: ${JSON.stringify({ name: "1 week from now" })}`]);
  });

  it("reads chip texts and clears them all", async () => {
    const clicks: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id !== "cycles-filter-chips") return loc();
        return loc({
          nested: (selector) => {
            void selector;
            const texts = ["Start date 1 week from now"];
            return loc({
              count: async () => texts.length,
              nth: (index) => loc({ innerText: async () => texts[index] ?? "" }),
            });
          },
          byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }),
        });
      },
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesFilterChipTexts()).toEqual(["Start date 1 week from now"]);
    await driver.archivesCyclesFiltersClearAll();
    expect(clicks).toEqual([JSON.stringify({ name: /clear all/i })]);
  });

  it("reads no chips when the row is absent", async () => {
    const { page } = stubPage({
      testIdFor: (id) => (id === "cycles-filter-chips" ? loc({ count: async () => 0 }) : loc()),
    });

    expect(await new WebNewDriver(page).archivesCyclesFilterChipTexts()).toEqual([]);
  });

  it("removes one chip through its group control", async () => {
    const clicks: string[] = [];
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id !== "cycles-filter-chips") return loc();
        return loc({
          nested: () =>
            loc({
              count: async () => 1,
              nth: () =>
                loc({
                  innerText: async () => "Start date 1 week from now",
                  byRole: () => loc({ click: async () => void clicks.push("dimension-clear") }),
                }),
            }),
        });
      },
    });

    await new WebNewDriver(page).archivesCyclesFilterRemoveChip("Start date 1 week from now");

    expect(clicks).toEqual(["dimension-clear"]);
  });

  it("reads the active-filters marker", async () => {
    const active = stubPage({});
    expect(await new WebNewDriver(active.page).archivesCyclesFiltersActive()).toBe(true);

    const idle = stubPage({
      testIdFor: (id) => (id === "cycles-filters-count" ? loc({ count: async () => 0 }) : loc()),
    });
    expect(await new WebNewDriver(idle.page).archivesCyclesFiltersActive()).toBe(false);
  });
});

describe("archived-cycles states", () => {
  it("reads the zero-state heading", async () => {
    const { page } = stubPage({
      roleFor: (role) => (role === "heading" ? loc({ innerText: async () => "  No archived cycles yet " }) : loc()),
    });

    expect(await new WebNewDriver(page).archivesCyclesEmptyHeading()).toBe("No archived cycles yet");
  });

  it("reads no heading when rows render", async () => {
    const { page } = stubPage({
      roleFor: (role) => (role === "heading" ? loc({ count: async () => 0 }) : loc()),
    });

    expect(await new WebNewDriver(page).archivesCyclesEmptyHeading()).toBe(null);
  });

  it("reads the no-match hint", async () => {
    const { page } = stubPage({
      testIdFor: (id) =>
        id === "cycles-no-match"
          ? loc({ nested: () => loc({ innerText: async () => "  Remove the filters to see all cycles " }) })
          : loc(),
    });

    expect(await new WebNewDriver(page).archivesCyclesNoMatchHint()).toBe("Remove the filters to see all cycles");
  });

  it("reads no hint when the list renders", async () => {
    const { page } = stubPage({
      testIdFor: (id) => (id === "cycles-no-match" ? loc({ count: async () => 0 }) : loc()),
    });

    expect(await new WebNewDriver(page).archivesCyclesNoMatchHint()).toBe(null);
  });

  it("reports the skeleton seen on a held fetch via the live screen", async () => {
    const { page, calls } = stubPage({
      url: "http://new.test/acme/projects/p1/cycles",
      evaluateImpl: () => true,
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesCyclesSkeletonShownOnSlowFetchViaLive("acme", "p1")).toBe(true);

    expect(calls.initScripts).toHaveLength(1);
    expect(calls.routes).toHaveLength(1);
    expect(calls.goto).toEqual(["/acme/projects/p1/cycles"]);
    expect(calls.unroutes).toEqual([calls.routes[0]?.matcher]);
    const matcher = calls.routes[0]?.matcher as (url: URL) => boolean;
    expect(matcher(new URL("http://new.test/api/workspaces/acme/projects/p1/archived-cycles/"))).toBe(true);
    expect(matcher(new URL("http://new.test/api/workspaces/acme/projects/p1/cycles/"))).toBe(false);
  });

  it("holds list reads briefly and lets anything else through", async () => {
    const { page, calls } = stubPage({ url: "http://new.test/acme/projects/p1/cycles" });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesSkeletonShownOnSlowFetchViaLive("acme", "p1");

    const handler = calls.routes[0]?.handler;
    if (!handler) throw new Error("no route handler armed");
    vi.useFakeTimers();
    try {
      const held = fakeRoute("GET", "http://new.test/api/workspaces/acme/projects/p1/archived-cycles/");
      const heldRun = handler(held.route);
      await vi.advanceTimersByTimeAsync(3_000);
      await heldRun;
      expect(held.continued()).toBe(true);
      const passed = fakeRoute("POST", "http://new.test/api/workspaces/acme/projects/p1/archived-cycles/");
      await handler(passed.route);
      expect(passed.continued()).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("archived-cycles row menus", () => {
  it("opens the named row's menu", async () => {
    const clicks: string[] = [];
    const { page } = stubPage({
      locatorFor: () =>
        loc({
          byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }),
        }),
    });

    await new WebNewDriver(page).archivesCyclesOpenRowMenu("Archived alpha");

    expect(clicks).toEqual([JSON.stringify({ name: "Cycle actions for Archived alpha" })]);
  });

  it("picks a menu entry by its heading", async () => {
    const clicks: string[] = [];
    const { page } = stubPage({
      roleFor: (role) =>
        role === "menuitem" ? loc({ filtered: () => loc({ click: async () => void clicks.push("picked") }) }) : loc(),
    });

    await new WebNewDriver(page).archivesCyclesMenuPick("Restore");

    expect(clicks).toEqual(["picked"]);
  });

  interface ScriptedItem {
    title: string | null;
    hint: string | null | "missing" | "throws";
    ariaDisabled: string | null | "throws";
    dataDisabled: string | null | "throws";
  }

  function itemDriver(items: ScriptedItem[]): WebNewDriver {
    const { page } = stubPage({
      roleFor: (role) => {
        if (role !== "menuitem") return loc();
        return loc({
          count: async () => items.length,
          nth: (index) => {
            const item = items[index];
            if (!item) throw new Error("no such item");
            const hintCount = item.hint === "missing" ? 0 : 1;
            return loc({
              nested: (selector) => {
                if (selector === "h5") {
                  return loc({
                    innerText: async () => {
                      if (item.title === null) throw new Error("detached");
                      return item.title;
                    },
                  });
                }
                return loc({
                  count: async () => hintCount,
                  innerText: async () => {
                    if (item.hint === "throws") throw new Error("detached");
                    if (item.hint === "missing" || item.hint === null) throw new Error("no hint");
                    return item.hint;
                  },
                });
              },
              getAttribute: async (name) => {
                const scripted = name === "aria-disabled" ? item.ariaDisabled : item.dataDisabled;
                if (scripted === "throws") throw new Error("detached");
                return scripted;
              },
            });
          },
        });
      },
    });
    return new WebNewDriver(page);
  }

  it("reads titles, disabled states and gate hints", async () => {
    const driver = itemDriver([
      { title: "Open in new tab", hint: "missing", ariaDisabled: null, dataDisabled: null },
      {
        title: "Archive",
        hint: "Only completed cycles can be archived",
        ariaDisabled: "true",
        dataDisabled: "true",
      },
      { title: "Restore", hint: "missing", ariaDisabled: null, dataDisabled: "disabled" },
    ]);

    expect(await driver.archivesCyclesMenuEntries()).toEqual([
      { title: "Open in new tab", disabled: false, description: null },
      { title: "Archive", disabled: true, description: "Only completed cycles can be archived" },
      { title: "Restore", disabled: true, description: null },
    ]);
  });

  it("degrades detached reads to empty titles and null hints", async () => {
    const driver = itemDriver([
      { title: null, hint: "throws", ariaDisabled: "throws", dataDisabled: "throws" },
      { title: "Copy link", hint: "", ariaDisabled: "false", dataDisabled: null },
    ]);

    expect(await driver.archivesCyclesMenuEntries()).toEqual([
      { title: "", disabled: false, description: null },
      { title: "Copy link", disabled: false, description: null },
    ]);
  });
});

describe("archived-cycles archive dialog", () => {
  it("reads the dialog heading plus its consequence copy", async () => {
    document.body.innerHTML =
      `<div role="dialog"><h2>Archive cycle Archived alpha</h2>` +
      `<p>"Archived alpha" will move to project archives. You can restore it at any time.</p></div>`;
    const heading = document.querySelector("h2");
    if (!heading) throw new Error("no heading");
    const calls: Array<{ fn: (element: never) => unknown }> = [];
    const dialogPage = stubPage({
      roleFor: (role) => {
        if (role !== "dialog") return loc();
        return loc({
          filtered: () =>
            loc({
              byRole: () =>
                loc({
                  innerText: async () => heading.textContent ?? "",
                  evaluate: (fn) => {
                    calls.push({ fn });
                    return fn(heading as never);
                  },
                }),
            }),
        });
      },
    });
    const found = await new WebNewDriver(dialogPage.page).archivesCyclesArchiveDialogText();

    expect(found?.heading).toBe("Archive cycle Archived alpha");
    expect(found?.body).toMatch(/restor/i);
    expect(calls).toHaveLength(1);
  });

  it("reads no dialog when none shows", async () => {
    const { page } = stubPage({
      waitForFunctionImpl: () => {
        throw new Error("timeout");
      },
    });

    expect(await new WebNewDriver(page).archivesCyclesArchiveDialogText()).toBe(null);
  });

  it("degrades detached dialog reads to empty copy", async () => {
    const { page } = stubPage({
      roleFor: (role) => {
        if (role !== "dialog") return loc();
        return loc({
          filtered: () =>
            loc({
              byRole: () =>
                loc({
                  innerText: async () => {
                    throw new Error("detached");
                  },
                  evaluate: () => {
                    throw new Error("detached");
                  },
                }),
            }),
        });
      },
    });

    expect(await new WebNewDriver(page).archivesCyclesArchiveDialogText()).toEqual({ heading: "", body: "" });
  });

  it("confirms once the archive write lands", async () => {
    const clicks: string[] = [];
    const { page, calls } = stubPage({
      roleFor: (role) => {
        if (role !== "dialog") return loc();
        return loc({
          filtered: () =>
            loc({
              byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }),
            }),
        });
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCyclesArchiveDialogConfirm();

    expect(clicks).toEqual([JSON.stringify({ name: "Archive", exact: true })]);
    expect(calls.waitForResponses).toHaveLength(1);
    const predicate = calls.waitForResponses[0]?.predicate as (response: never) => boolean;
    expect(predicate(fakeResponse("POST", "http://new.test/api/workspaces/acme/projects/p1/cycles/c1/archive/"))).toBe(
      true
    );
    expect(predicate(fakeResponse("GET", "http://new.test/api/workspaces/acme/projects/p1/cycles/"))).toBe(false);
    expect(predicate(fakeResponse("POST", "http://new.test/api/workspaces/acme/projects/p1/cycles/"))).toBe(false);
  });

  it("cancels and waits for the dialog to close", async () => {
    const clicks: string[] = [];
    const waits: unknown[] = [];
    const { page } = stubPage({
      roleFor: (role) => {
        if (role !== "dialog") return loc();
        return loc({
          filtered: () =>
            loc({
              waitFor: async (options) => void waits.push(options),
              byRole: (_role, options) => loc({ click: async () => void clicks.push(JSON.stringify(options)) }),
            }),
        });
      },
    });

    await new WebNewDriver(page).archivesCyclesArchiveDialogCancel();

    expect(clicks).toEqual([JSON.stringify({ name: "Cancel", exact: true })]);
    expect(waits).toEqual([{ state: "detached", timeout: 30_000 }]);
  });
});
