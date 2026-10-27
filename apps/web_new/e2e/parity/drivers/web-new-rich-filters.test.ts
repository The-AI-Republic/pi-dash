// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver rich-filter methods (NEWFRONT-658,
// ISS-095–105). A stub Page stands in for Playwright: each case pins the
// selector or value the method reads back from the rich-filter builder, so
// a markup change that breaks the parity hooks fails here before the parity
// stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  all?: () => Promise<Locator[]>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  or?: (other: StubLocator) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isDisabled: async () => false,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    allTextContents: async () => [],
    all: async () => [],
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    or: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(
  parts: {
    testId?: (id: string) => Locator;
    role?: (role: string, options?: unknown) => Locator;
    locator?: (selector: string) => Locator;
    tick?: () => void;
  } = {}
): Page {
  return {
    getByTestId: parts.testId ?? (() => locator()),
    getByRole: parts.role ?? (() => locator()),
    locator: parts.locator ?? (() => locator()),
    waitForTimeout: async () => {
      parts.tick?.();
    },
  } as unknown as Page;
}

/** A row whose only special child is its Add filter control. */
function rowWithAdd(onClick: () => void, last = true): Locator {
  return locator({
    getByRole: (role, options) =>
      role === "button" && (options as { name?: string } | undefined)?.name === "Add filter"
        ? locator({ click: async () => onClick(), last: () => locator({ click: async () => onClick() }) })
        : locator(),
    getByTestId: () => locator(last ? { click: async () => onClick() } : {}),
  });
}

describe("rich filter row", () => {
  it("reports the row hidden when the builder never mounted one", async () => {
    const page = pageOf({ testId: () => locator({ count: async () => 0 }) });
    expect(await new WebNewDriver(page).isRichFilterRowVisible()).toBe(false);
    expect(await new WebNewDriver(page).richFilterRowText()).toBe("");
  });

  it("reports the row visible from the mounted builder", async () => {
    const page = pageOf({ testId: () => locator({ isVisible: async () => true }) });
    expect(await new WebNewDriver(page).isRichFilterRowVisible()).toBe(true);
  });

  it("reads the row text from the visible builder", async () => {
    const page = pageOf({ testId: () => locator({ innerText: async () => "  Priority is Urgent " }) });
    expect(await new WebNewDriver(page).richFilterRowText()).toBe("Priority is Urgent");
  });

  it("counts one remove control per condition", async () => {
    const removes = locator({ count: async () => 3 });
    const page = pageOf({ testId: () => locator({ getByRole: () => removes }) });
    expect(await new WebNewDriver(page).richConditionCount()).toBe(3);
  });

  it("clicks the nth remove control for the requested index", async () => {
    const clicks: string[] = [];
    const removes = locator({
      nth: (index: number) => locator({ click: async () => void clicks.push(String(index)) }),
    });
    const page = pageOf({ testId: () => locator({ getByRole: () => removes }) });
    await new WebNewDriver(page).removeRichCondition(2);
    expect(clicks).toEqual(["2"]);
  });

  it("clicks the header toggle", async () => {
    const clicks: string[] = [];
    const page = pageOf({
      testId: (id) =>
        id === "rich-filter-toggle" ? locator({ click: async () => void clicks.push("toggle") }) : locator(),
    });
    await new WebNewDriver(page).toggleRichFilterRow();
    expect(clicks).toEqual(["toggle"]);
  });

  it("clicks clear-all inside the row", async () => {
    const clicks: string[] = [];
    const clear = locator({ click: async () => void clicks.push("clear") });
    const page = pageOf({
      testId: (id) =>
        id === "rich-filter-row"
          ? locator({ getByTestId: (nested) => (nested === "rich-clear-all" ? clear : locator()) })
          : locator(),
    });
    await new WebNewDriver(page).clearRichFilters();
    expect(clicks).toEqual(["clear"]);
  });
});

describe("rich filter picker", () => {
  it("lists the field options and closes the picker again", async () => {
    const clicks: string[] = [];
    const options = locator({ allTextContents: async () => [" State ", "Priority", ""] });
    const page = pageOf({
      role: (role) => (role === "option" ? options : locator()),
      testId: (id) => (id === "rich-filter-row" ? rowWithAdd(() => clicks.push("add")) : locator()),
    });
    // Options are still on screen, so the helper toggles the add control
    // shut after reading them.
    expect(await new WebNewDriver(page).listRichPickerOptions()).toEqual(["State", "Priority"]);
    expect(clicks).toEqual(["add", "add"]);
  });

  it("picks the named field and waits for the condition count to grow", async () => {
    let count = 1;
    const clicks: string[] = [];
    const page = pageOf({
      role: (role, options) => {
        if (role !== "option") return locator();
        if ((options as { name?: string } | undefined)?.name === "State") {
          return locator({ click: async () => void ((count += 1), clicks.push("State")) });
        }
        return locator();
      },
      testId: (id) => (id === "rich-filter-row" ? rowWithAdd(() => clicks.push("add")) : locator()),
    });
    // richConditionCount reads the remove controls; make it mirror `count`.
    const driver = new WebNewDriver(page);
    Object.defineProperty(driver, "richConditionCount", { value: async () => count });
    await driver.addRichCondition("State");
    expect(clicks).toContain("State");
  });
});

describe("rich filter values", () => {
  it("reads value options from the rich value sections only", async () => {
    let scopedSelector: string | null = null;
    const page = pageOf({
      locator: (selector) => {
        scopedSelector = selector;
        return locator({ getByRole: () => locator({ allTextContents: async () => [" Urgent ", "High"] }) });
      },
    });
    expect(await new WebNewDriver(page).richValueOptions()).toEqual(["Urgent", "High"]);
    expect(scopedSelector).toBe('[data-testid^="rich-section-"]');
  });

  it("opens a shut value slot before picking", async () => {
    const clicks: string[] = [];
    let reads = 0;
    const choices = locator({
      count: async () => {
        reads += 1;
        return reads === 1 ? 0 : 1;
      },
      first: () => locator({ waitFor: async () => undefined }),
      filter: () => locator({ first: () => locator({ click: async () => void clicks.push("picked") }) }),
    });
    const page = pageOf({
      locator: () => locator({ getByRole: () => choices }),
      testId: (id) =>
        id === "rich-filter-row"
          ? locator({
              getByTestId: (nested) =>
                nested === "rich-value-trigger"
                  ? locator({ click: async () => void clicks.push("open-slot") })
                  : locator(),
            })
          : locator(),
    });
    await new WebNewDriver(page).pickRichValues(["Urgent"]);
    expect(clicks).toContain("open-slot");
    expect(clicks).toContain("picked");
  });
});

describe("rich filter dates", () => {
  it("opens the calendar when the slot was shut and clicks the in-month day", async () => {
    const clicks: string[] = [];
    let calendarReads = 0;
    const page = pageOf({
      locator: (selector) => {
        if (selector.includes("data-day")) return locator({ click: async () => void clicks.push("day") });
        calendarReads += 1;
        return locator({ count: async () => (calendarReads === 1 ? 0 : 1), isVisible: async () => false });
      },
      testId: (id) =>
        id === "rich-filter-row"
          ? locator({
              getByTestId: (nested) =>
                nested === "rich-value-trigger" ? locator({ click: async () => void clicks.push("slot") }) : locator(),
            })
          : locator(),
    });
    await new WebNewDriver(page).pickRichDay("15");
    expect(clicks).toContain("slot");
    expect(clicks).toContain("day");
  });

  it("reports the calendar closed when neither testid is mounted", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    expect(await new WebNewDriver(page).isRichCalendarOpen()).toBe(false);
  });
});

describe("rich filter operators", () => {
  /** A single pill whose name-less, text-bearing buttons are [property, operator]. */
  function pillPage(operator: StubLocator): Page {
    const property = locator({ innerText: async () => "Priority" });
    return pageOf({
      testId: (id) =>
        id === "rich-filter-row"
          ? locator({
              getByTestId: () => locator({ getByRole: () => locator({ all: async () => [property, operator] }) }),
            })
          : locator(),
    });
  }

  it("reads the lone condition's operator lock", async () => {
    const page = pillPage(locator({ isDisabled: async () => true, innerText: async () => "is" }));
    expect(await new WebNewDriver(page).isSingleRichOperatorLocked()).toBe(true);
  });

  it("lists operator options and closes the combobox again", async () => {
    const clicks: string[] = [];
    const operator = locator({
      innerText: async () => "is",
      click: async () => void clicks.push("op"),
    });
    const options = locator({ allTextContents: async () => [" is ", "is not", "between"] });
    const page = pageOf({
      ...{
        testId: (id: string) =>
          id === "rich-filter-row"
            ? locator({
                getByTestId: () =>
                  locator({
                    getByRole: () =>
                      locator({ all: async () => [locator({ innerText: async () => "Priority" }), operator] }),
                  }),
              })
            : locator(),
      },
      role: (role) => (role === "option" ? options : locator()),
    });
    expect(await new WebNewDriver(page).richOperatorOptions()).toEqual(["is", "is not", "between"]);
    expect(clicks).toEqual(["op", "op"]);
  });

  it("picks the named operator", async () => {
    const clicks: string[] = [];
    const operator = locator({ innerText: async () => "is", click: async () => void clicks.push("open") });
    const page = pageOf({
      testId: (id) =>
        id === "rich-filter-row"
          ? locator({
              getByTestId: () =>
                locator({
                  getByRole: () =>
                    locator({ all: async () => [locator({ innerText: async () => "Priority" }), operator] }),
                }),
            })
          : locator(),
      role: (role, options) =>
        role === "option" && (options as { name?: string } | undefined)?.name === "between"
          ? locator({ click: async () => void clicks.push("between") })
          : locator(),
    });
    await new WebNewDriver(page).pickRichOperator("between");
    expect(clicks).toEqual(["open", "between"]);
  });
});

describe("rich filter add-condition", () => {
  interface AddConditionHooks {
    popup: () => boolean;
    checkbox: () => boolean;
    viewAll: () => boolean;
    onViewAll?: () => void;
    onPlaceholder?: () => void;
    clicks: string[];
    tick?: () => void;
  }

  function addConditionPage(hooks: AddConditionHooks): Page {
    return pageOf({
      role: (role, options) => {
        const name = (options as { name?: unknown } | undefined)?.name;
        if (role === "button" && name === "Remove filter") return locator();
        if (role === "button" && name === "Add filter") {
          return locator({ click: async () => void hooks.clicks.push("add") });
        }
        if (role === "option") {
          return locator({ click: async () => void hooks.clicks.push(`property:${String(name)}`) });
        }
        if (role === "button" && name === "--") {
          return locator({
            click: async () => {
              hooks.clicks.push("placeholder");
              hooks.onPlaceholder?.();
            },
          });
        }
        if (role === "button" && name === "View all") {
          if (!hooks.viewAll()) return locator({ count: async () => 0 });
          return locator({
            click: async () => {
              hooks.clicks.push("view-all");
              hooks.onViewAll?.();
            },
          });
        }
        if (role === "checkbox") {
          if (!hooks.checkbox()) return locator({ count: async () => 0 });
          return locator({ click: async () => void hooks.clicks.push(`value:${String(name)}`) });
        }
        return locator({ count: async () => 0 });
      },
      locator: (selector) =>
        selector === '[data-testid^="rich-section-"]'
          ? locator({ count: async () => (hooks.popup() ? 1 : 0) })
          : locator({ count: async () => 0 }),
      tick: hooks.tick,
    });
  }

  function tickedClock(): { tick: () => void; restore: () => void } {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    return {
      tick: () => {
        now += 60_000;
      },
      restore: () => spy.mockRestore(),
    };
  }

  it("picks the value without helpers when the popup auto-opens", async () => {
    const clicks: string[] = [];
    const page = addConditionPage({
      popup: () => true,
      checkbox: () => true,
      viewAll: () => false,
      clicks,
    });
    await new WebNewDriver(page).layoutsFilterAddConditionViaRow("State group", "Started");
    expect(clicks).toEqual(["add", "property:State group", "value:Started"]);
  });

  it("opens the value list by hand when the popup stays closed", async () => {
    const clock = tickedClock();
    try {
      const clicks: string[] = [];
      let popup = false;
      const page = addConditionPage({
        popup: () => popup,
        checkbox: () => true,
        viewAll: () => false,
        onPlaceholder: () => void (popup = true),
        clicks,
        tick: clock.tick,
      });
      await new WebNewDriver(page).layoutsFilterAddConditionViaRow("State", "Todo");
      expect(clicks).toEqual(["add", "property:State", "placeholder", "value:Todo"]);
    } finally {
      clock.restore();
    }
  });

  it("expands a capped list before picking", async () => {
    const clicks: string[] = [];
    let revealed = false;
    const page = addConditionPage({
      popup: () => true,
      checkbox: () => revealed,
      viewAll: () => !revealed,
      onViewAll: () => void (revealed = true),
      clicks,
    });
    await new WebNewDriver(page).layoutsFilterAddConditionViaRow("State", "Parity EFilt x");
    expect(clicks).toEqual(["add", "property:State", "view-all", "value:Parity EFilt x"]);
  });

  it("expands at most once even when the button sticks around", async () => {
    const clock = tickedClock();
    try {
      const clicks: string[] = [];
      const page = addConditionPage({
        popup: () => true,
        checkbox: () => false,
        viewAll: () => true,
        clicks,
        tick: clock.tick,
      });
      await expect(new WebNewDriver(page).layoutsFilterAddConditionViaRow("State", "Missing")).rejects.toThrow(
        "[parity] no filter value Missing."
      );
      expect(clicks.filter((entry) => entry === "view-all")).toHaveLength(1);
    } finally {
      clock.restore();
    }
  });

  it("throws when no value ever appears", async () => {
    const clock = tickedClock();
    try {
      const page = addConditionPage({
        popup: () => true,
        checkbox: () => false,
        viewAll: () => false,
        clicks: [],
        tick: clock.tick,
      });
      await expect(new WebNewDriver(page).layoutsFilterAddConditionViaRow("State group", "Started")).rejects.toThrow(
        "[parity] no filter value Started."
      );
    } finally {
      clock.restore();
    }
  });

  it("throws when no pill is seeded", async () => {
    const clock = tickedClock();
    try {
      const nopills = pageOf({
        role: (role, options) => {
          if (role === "button" && (options as { name?: unknown } | undefined)?.name === "Remove filter") {
            return locator({ count: async () => 0 });
          }
          return locator();
        },
        tick: clock.tick,
      });
      await expect(new WebNewDriver(nopills).layoutsFilterAddConditionViaRow("State group", "Started")).rejects.toThrow(
        "[parity] no filter pill: seed one before adding a condition."
      );
    } finally {
      clock.restore();
    }
  });
});

describe("saveRichViewAs / updateRichView", () => {
  function savePage() {
    const saveClick = vi.fn(async () => undefined);
    const createClick = vi.fn(async () => undefined);
    const fill = vi.fn(async () => undefined);
    const waits: unknown[] = [];
    const field = locator({ fill, waitFor: async () => undefined });
    const dialog = locator({
      getByPlaceholder: () => field,
      getByRole: () => locator({ click: createClick }),
      waitFor: async (options?: unknown) => {
        waits.push(options);
      },
    });
    const page = {
      getByTestId: (id: string) => (id === "rich-save-view" ? locator({ click: saveClick }) : locator()),
      getByRole: () => dialog,
      getByPlaceholder: () => locator(),
    } as unknown as Page;
    return { driver: new WebNewDriver(page), saveClick, createClick, fill, waits };
  }

  it("saves through the dialog and waits for it to close", async () => {
    const { driver, saveClick, createClick, fill, waits } = savePage();
    await driver.saveRichViewAs("Oracle saved view");
    expect(saveClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
    expect(fill).toHaveBeenCalledWith("Oracle saved view");
    expect(createClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
    expect(waits).toContainEqual({ state: "detached", timeout: 60_000 });
  });

  it("waits generously for the Update view action", async () => {
    const updateClick = vi.fn(async () => undefined);
    const waits: unknown[] = [];
    const actions = locator({
      getByRole: () =>
        locator({
          click: updateClick,
          waitFor: async (options?: unknown) => {
            waits.push(options);
          },
        }),
    });
    const page = { getByTestId: () => actions } as unknown as Page;
    await new WebNewDriver(page).updateRichView();
    expect(waits).toContainEqual({ state: "visible", timeout: 60_000 });
    expect(updateClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });
});

describe("openProjectView", () => {
  function viewPage(counts: { row?: number; switcher?: number; email?: number } = {}) {
    const goto = vi.fn(async () => undefined);
    const page = {
      goto,
      getByTestId: (id: string | RegExp) =>
        typeof id === "string" && id === "rich-filter-row"
          ? locator({ count: async () => counts.row ?? 0 })
          : locator({ count: async () => counts.switcher ?? 0 }),
      getByPlaceholder: () => locator({ count: async () => counts.email ?? 0 }),
      waitForTimeout: async () => undefined,
    } as unknown as Page;
    return { driver: new WebNewDriver(page), goto };
  }

  it("opens the view URL and settles on the filter row", async () => {
    const { driver, goto } = viewPage({ row: 1 });
    await driver.openProjectView("ws", "pid", "vid");
    expect(goto).toHaveBeenCalledWith("/ws/projects/pid/views/vid", { waitUntil: "domcontentloaded" });
  });

  it("settles on a layout twin when the row stays hidden", async () => {
    const { driver, goto } = viewPage({ switcher: 1 });
    await driver.openProjectView("ws", "pid", "vid");
    expect(goto).toHaveBeenCalledTimes(1);
  });

  it("throws when the view bounces to sign-in", async () => {
    const { driver } = viewPage({ email: 1 });
    await expect(driver.openProjectView("ws", "pid", "vid")).rejects.toThrow("bounced to sign-in");
  });

  it("gives up settling after a minute without throwing", async () => {
    const { driver } = viewPage({});
    let calls = 0;
    const now = vi.spyOn(Date, "now").mockImplementation(() => {
      calls += 1;
      return calls <= 1 ? 1_000 : 1_000 + 61_000;
    });
    try {
      await driver.openProjectView("ws", "pid", "vid");
    } finally {
      now.mockRestore();
    }
  });
});

describe("rich write settle", () => {
  it("remove waits out the post-write hidden window", async () => {
    const clicks: string[] = [];
    let visibleCalls = 0;
    const row = locator({
      getByRole: () => locator({ nth: () => locator({ click: async () => void clicks.push("remove") }) }),
      isVisible: async () => {
        visibleCalls += 1;
        return visibleCalls > 1;
      },
    });
    const page = pageOf({ testId: () => row });
    await new WebNewDriver(page).removeRichCondition(0);
    expect(clicks).toEqual(["remove"]);
    expect(visibleCalls).toBeGreaterThanOrEqual(3);
  });

  it("clear settles on the absent row", async () => {
    let counts = 0;
    const row = locator({
      getByTestId: () => locator({ click: async () => undefined }),
      count: async () => {
        counts += 1;
        return counts > 1 ? 0 : 1;
      },
    });
    const page = pageOf({ testId: () => row });
    await new WebNewDriver(page).clearRichFilters();
    expect(counts).toBeGreaterThanOrEqual(3);
  });

  it("settle gives up silently when the row never stabilizes", async () => {
    const row = locator({ isVisible: async () => false });
    const page = pageOf({ testId: () => row });
    // Construct before mocking time: the driver's startedAt initializer
    // consumes a Date.now call at construction.
    const driver = new WebNewDriver(page);
    let calls = 0;
    const now = vi.spyOn(Date, "now").mockImplementation(() => {
      calls += 1;
      return calls <= 1 ? 1_000 : 1_000 + 31_000;
    });
    try {
      await driver.clearRichFilters();
    } finally {
      now.mockRestore();
    }
  });
});
