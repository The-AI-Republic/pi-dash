// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver rich-filter methods (NEWFRONT-658,
// ISS-095–105). A stub Page stands in for Playwright: each case pins the
// selector or value the method reads back from the rich-filter builder, so
// a markup change that breaks the parity hooks fails here before the parity
// stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

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
  } = {}
): Page {
  return {
    getByTestId: parts.testId ?? (() => locator()),
    getByRole: parts.role ?? (() => locator()),
    locator: parts.locator ?? (() => locator()),
    waitForTimeout: async () => undefined,
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
