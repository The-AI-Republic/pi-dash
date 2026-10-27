// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver spreadsheet methods (NEWFRONT-658,
// ISS-015–020). A stub Page records the selectors each method reads, so
// dropping the markup hooks the sheet scenarios depend on fails here
// before the parity stack.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  getAttribute?: (name: string) => Promise<string | null>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  press?: (key: string) => Promise<void>;
  focus?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  selectOption?: (option: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  evaluate?: (fn: (node: never) => unknown) => Promise<unknown>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByPlaceholder?: (text: string) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  last?: () => Locator;
  nth?: (index: number) => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    innerText: async () => "",
    inputValue: async () => "",
    getAttribute: async () => null,
    isVisible: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    press: async () => undefined,
    focus: async () => undefined,
    fill: async () => undefined,
    selectOption: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    waitFor: async () => undefined,
    evaluate: async () => undefined,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node(),
    getByTestId: () => node(),
    getByRole: () => node(),
    getByPlaceholder: () => node(),
    filter: () => node(),
    first: () => self as unknown as Locator,
    last: () => self as unknown as Locator,
    nth: () => self as unknown as Locator,
  };
  for (const [name, value] of Object.entries(chain)) {
    if (self[name as keyof StubNode] === undefined) {
      (self as Record<string, unknown>)[name] = value;
    }
  }
  return self as unknown as Locator;
}

function collection(items: Locator[]): Locator {
  return node({
    count: async () => items.length,
    nth: (index: number) => items[index] ?? node({ count: async () => 0 }),
    first: () => items[0] ?? node({ count: async () => 0 }),
    last: () => items[items.length - 1] ?? node({ count: async () => 0 }),
  });
}

interface PageRoutes {
  locator?: (selector: string) => Locator;
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  placeholder?: (text: string) => Locator;
  keyboardPress?: (key: string) => Promise<void>;
  evaluate?: (fn: (arg: never) => unknown) => Promise<unknown>;
  waitForURL?: (fn: (url: URL) => boolean, options?: unknown) => Promise<void>;
  tick?: () => void;
}

interface PageStub {
  page: Page;
  selectors: string[];
  testIds: string[];
  roles: Array<{ role: string; name: unknown }>;
  keys: string[];
  urlPredicates: Array<(url: URL) => boolean>;
}

function pageOf(routes: PageRoutes = {}): PageStub {
  const state: PageStub = {
    page: undefined as unknown as Page,
    selectors: [],
    testIds: [],
    roles: [],
    keys: [],
    urlPredicates: [],
  };
  state.page = {
    url: () => "https://app.test/ws/issues",
    locator: (selector: string) => {
      state.selectors.push(selector);
      return routes.locator?.(selector) ?? node({ count: async () => 0 });
    },
    getByTestId: (id: string) => {
      state.testIds.push(id);
      return routes.testId?.(id) ?? node({ count: async () => 0 });
    },
    getByRole: (role: string, options?: unknown) => {
      state.roles.push({ role, name: (options as { name?: unknown } | undefined)?.name });
      return routes.role?.(role, options) ?? node({ count: async () => 0 });
    },
    getByPlaceholder: (text: string) => routes.placeholder?.(text) ?? node(),
    keyboard: {
      press: async (key: string) => {
        state.keys.push(key);
        await routes.keyboardPress?.(key);
      },
    },
    context: () => ({
      grantPermissions: async () => undefined,
      waitForEvent: async () => ({ url: () => "", close: async () => undefined }),
    }),
    evaluate: (fn: (arg: never) => unknown) => {
      if (routes.evaluate !== undefined) return routes.evaluate(fn);
      return Promise.resolve(undefined);
    },
    waitForURL: async (fn: (url: URL) => boolean, options?: unknown) => {
      state.urlPredicates.push(fn);
      await routes.waitForURL?.(fn, options);
    },
    waitForTimeout: async () => {
      routes.tick?.();
    },
    reload: async () => null,
  } as unknown as Page;
  return state;
}

/** A first-column cell showing `name` through a title link. */
function firstCell(name: string): Locator {
  return node({
    locator: (selector: string) =>
      selector === "a" ? node({ innerText: async () => name }) : node({ count: async () => 0 }),
  });
}

/** A table whose first column reads `names`, for row scans. */
function sheetTable(names: string[]): Locator {
  return node({
    locator: (selector: string) => {
      if (selector === 'tbody td[id^="issue-"]') return collection(names.map((name) => firstCell(name)));
      if (selector === "tbody tr") {
        return collection(
          names.map((name) =>
            node({
              getAttribute: async () => "0",
              locator: () => firstCell(name),
            })
          )
        );
      }
      return node({ count: async () => 0 });
    },
  });
}

function sheetRoutes(table: Locator): Pick<PageRoutes, "testId"> {
  return {
    testId: (id: string) =>
      id === "spreadsheet-scroller"
        ? node({ locator: (selector: string) => (selector === "table" ? table : node()) })
        : node({ count: async () => 0 }),
  };
}

describe("web_new sheet structure", () => {
  it("reads visibility from the scroller", async () => {
    const shown = pageOf(sheetRoutes(sheetTable([])));
    await expect(new WebNewDriver(shown.page).layoutsSpreadsheetVisible()).resolves.toBe(true);
    const missing = pageOf({});
    await expect(new WebNewDriver(missing.page).layoutsSpreadsheetVisible()).resolves.toBe(false);
  });

  it("reads header labels and drops blanks", async () => {
    const table = node({
      locator: (selector: string) =>
        selector === "thead th"
          ? collection(["Work items", "  State  ", ""].map((text) => node({ innerText: async () => text })))
          : node({ count: async () => 0 }),
    });
    const stub = pageOf(sheetRoutes(table));
    await expect(new WebNewDriver(stub.page).layoutsSheetHeaders()).resolves.toEqual(["Work items", "State"]);
  });

  it("reads row names through links, spans, and blanks", async () => {
    const linked = node({
      locator: (selector: string) =>
        selector === "a" ? node({ innerText: async () => "Alpha" }) : node({ count: async () => 0 }),
    });
    const optimistic = node({
      locator: (selector: string) =>
        selector === "div.truncate" ? node({ innerText: async () => "Beta" }) : node({ count: async () => 0 }),
    });
    const bare = node({ locator: () => node({ count: async () => 0 }) });
    const table = node({
      locator: (selector: string) =>
        selector === 'tbody td[id^="issue-"]'
          ? collection([linked, optimistic, bare, node({ count: async () => 0 })])
          : node({ count: async () => 0 }),
    });
    // The empty locator covers the missing-cell guard through nth past
    // the end; collection past-the-end already reads blank the same way.
    const stub = pageOf(sheetRoutes(table));
    await expect(new WebNewDriver(stub.page).layoutsSheetRowNames()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("reads stickiness from computed position", async () => {
    const previous = (globalThis as { getComputedStyle?: unknown }).getComputedStyle;
    (globalThis as { getComputedStyle?: unknown }).getComputedStyle = () => ({ position: "sticky" });
    try {
      const table = node({
        locator: () => node({ evaluate: async (fn) => (fn as (n: unknown) => unknown)({}) }),
      });
      const stub = pageOf(sheetRoutes(table));
      const driver = new WebNewDriver(stub.page);
      await expect(driver.layoutsSheetFirstColumnSticky()).resolves.toBe(true);
      await expect(driver.layoutsSheetHeaderSticky()).resolves.toBe(true);
    } finally {
      (globalThis as { getComputedStyle?: unknown }).getComputedStyle = previous;
    }
    const other = (globalThis as { getComputedStyle?: unknown }).getComputedStyle;
    (globalThis as { getComputedStyle?: unknown }).getComputedStyle = () => ({ position: "static" });
    try {
      const table = node({
        locator: () => node({ evaluate: async (fn) => (fn as (n: unknown) => unknown)({}) }),
      });
      const stub = pageOf(sheetRoutes(table));
      await expect(new WebNewDriver(stub.page).layoutsSheetFirstColumnSticky()).resolves.toBe(false);
    } finally {
      (globalThis as { getComputedStyle?: unknown }).getComputedStyle = other;
    }
  });

  it("reads the first-column shadow from its inline style", async () => {
    for (const [shadow, expected] of [
      ["8px 22px 22px 10px rgba(0, 0, 0, 0.05)", true],
      ["none", false],
      ["", false],
    ] as Array<[string, boolean]>) {
      const cell = node({ evaluate: async (fn) => (fn as (n: unknown) => unknown)({ style: { boxShadow: shadow } }) });
      const table = node({
        locator: (selector: string) =>
          selector === 'tbody td[id^="issue-"]' ? collection([cell]) : node({ count: async () => 0 }),
      });
      const stub = pageOf(sheetRoutes(table));
      await expect(new WebNewDriver(stub.page).layoutsSheetFirstColumnShadowed()).resolves.toBe(expected);
    }
  });

  it("scrolls the scroller to its far edges", async () => {
    const scrolled: string[] = [];
    const scroller = node({
      evaluate: async (fn) =>
        (fn as (n: unknown) => unknown)({
          scrollWidth: 900,
          scrollHeight: 1200,
          scrollTo: (options: unknown) => void scrolled.push(JSON.stringify(options)),
        }),
    });
    const stub = pageOf({ testId: () => scroller });
    const driver = new WebNewDriver(stub.page);
    await driver.layoutsSheetScrollRight();
    await driver.layoutsSheetScrollEnd();
    expect(scrolled).toEqual([JSON.stringify({ left: 900 }), JSON.stringify({ top: 1200 })]);
  });
});

describe("web_new sheet cells", () => {
  /** Routes the one addressed cell; records its selector. */
  function cellRoutes(cell: Locator): Pick<PageRoutes, "locator"> {
    return { locator: () => cell };
  }

  it("addresses cells by column and issue, escaping quotes", async () => {
    const cell = node({ innerText: async () => "Todo" });
    const stub = pageOf(cellRoutes(cell));
    await expect(new WebNewDriver(stub.page).layoutsSheetCellText('Al"pha', "State")).resolves.toBe("Todo");
    expect(stub.selectors).toEqual(['td[aria-label="State for Al\\"pha"]']);
  });

  it("collapses cell whitespace", async () => {
    const cell = node({ innerText: async () => "  High\npriority  " });
    const stub = pageOf(cellRoutes(cell));
    await expect(new WebNewDriver(stub.page).layoutsSheetCellText("Alpha", "Priority")).resolves.toBe("High priority");
  });

  it("reads editability from the trigger controls", async () => {
    const editable = pageOf(cellRoutes(node()));
    await expect(new WebNewDriver(editable.page).layoutsSheetCellEditable("Alpha", "State")).resolves.toBe(true);
    const readonly = pageOf(cellRoutes(node({ locator: () => node({ count: async () => 0 }) })));
    await expect(new WebNewDriver(readonly.page).layoutsSheetCellEditable("Alpha", "Created on")).resolves.toBe(false);
  });

  it("picks state, priority, and assignee through the listbox", async () => {
    for (const [method, column, option] of [
      ["layoutsSheetCellSetState", "State", "Parity Sheet"],
      ["layoutsSheetCellSetPriority", "Priority", "High"],
      ["layoutsSheetCellSetAssignee", "Assignees", "Parity Member"],
    ] as const) {
      const picked: unknown[] = [];
      const keys: string[] = [];
      const states: unknown[] = [];
      const listbox = node({
        waitFor: async (options?: unknown) => {
          states.push((options as { state?: unknown } | undefined)?.state);
        },
        getByRole: (role: string, options?: unknown) => {
          if (role === "option") picked.push((options as { name?: unknown } | undefined)?.name);
          return node();
        },
      });
      const cell = node({
        locator: () => node(),
      });
      const stub = pageOf({
        locator: () => cell,
        role: (role: string) => (role === "listbox" ? listbox : node({ count: async () => 0 })),
        keyboardPress: async (key: string) => void keys.push(key),
      });
      await (new WebNewDriver(stub.page)[method] as (issue: string, value: string) => Promise<void>)("Alpha", option);
      expect(stub.selectors).toEqual([`td[aria-label="${column} for Alpha"]`]);
      expect(picked).toEqual([option]);
      expect(keys).toEqual(["Escape"]);
      expect(states).toEqual(["visible", "hidden"]);
    }
  });

  it("focuses cells directly and moves with arrows", async () => {
    let focused = 0;
    const cell = node({ focus: async () => void (focused += 1) });
    const stub = pageOf(cellRoutes(cell));
    const driver = new WebNewDriver(stub.page);
    await driver.layoutsSheetFocusCell("Alpha", "State");
    expect(focused).toBe(1);
    await driver.layoutsSheetPressArrow("up");
    await driver.layoutsSheetPressArrow("down");
    await driver.layoutsSheetPressArrow("left");
    await driver.layoutsSheetPressArrow("right");
    expect(stub.keys).toEqual(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);
  });

  it("reads the focused cell from its label", async () => {
    const cases: Array<[unknown, { issueName: string; column: string } | null]> = [
      ["State for Parity first issue", { issueName: "Parity first issue", column: "State" }],
      ["State for Cry for help", { issueName: "Cry for help", column: "State" }],
      ["Work items", null],
      [null, null],
    ];
    for (const [label, expected] of cases) {
      const previous = (globalThis as { document?: unknown }).document;
      (globalThis as { document?: unknown }).document = {
        activeElement: { closest: () => ({ getAttribute: () => label }) },
      };
      try {
        const stub = pageOf({ evaluate: async (fn) => (fn as () => unknown)() });
        await expect(new WebNewDriver(stub.page).layoutsSheetFocusedCell()).resolves.toEqual(expected);
      } finally {
        (globalThis as { document?: unknown }).document = previous;
      }
    }
    const unfocused = pageOf({ evaluate: async (fn) => (fn as () => unknown)() });
    const previous = (globalThis as { document?: unknown }).document;
    (globalThis as { document?: unknown }).document = { activeElement: null };
    try {
      await expect(new WebNewDriver(unfocused.page).layoutsSheetFocusedCell()).resolves.toBeNull();
    } finally {
      (globalThis as { document?: unknown }).document = previous;
    }
  });
});

describe("web_new sheet sorting", () => {
  /** A header cell; records whether its button was clicked. */
  function headerCell(clicked: { count: number }): Locator {
    return node({
      getByRole: (role: string) =>
        role === "button" ? node({ click: async () => void (clicked.count += 1) }) : node(),
    });
  }

  function headerRoutes(header: Locator, filters: unknown[]): Pick<PageRoutes, "locator"> {
    return {
      locator: (selector: string) => {
        if (selector !== "thead th") return node({ count: async () => 0 });
        return node({
          filter: (options: unknown) => {
            filters.push(options);
            return node({ first: () => header });
          },
        });
      },
    };
  }

  function menuItem(label: string, clicks: string[]): Locator {
    return node({
      innerText: async () => label,
      locator: () => node({ count: async () => 0 }),
      click: async () => void clicks.push(label),
    });
  }

  it("reads the sort menu and dismisses it", async () => {
    const clicked = { count: 0 };
    const filters: unknown[] = [];
    const stub = pageOf({
      ...headerRoutes(headerCell(clicked), filters),
      role: (role: string) =>
        role === "menuitem"
          ? collection(["Ascending", "Descending"].map((label) => menuItem(label, [])))
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsSheetSortMenu("State")).resolves.toEqual([
      "Ascending",
      "Descending",
    ]);
    expect(clicked.count).toBe(1);
    expect(filters).toEqual([{ hasText: "State" }]);
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("sorts and clears through the menu entries", async () => {
    for (const [method, args, picked] of [
      ["layoutsSheetSort", ["State", "ascending"], "Ascending"],
      ["layoutsSheetSort", ["State", "descending"], "Descending"],
      ["layoutsSheetClearSort", ["State"], "Clear sorting"],
    ] as const) {
      const labels = ["Ascending", "Descending", "Clear sorting"];
      const clicks: string[] = [];
      const clicked = { count: 0 };
      const stub = pageOf({
        ...headerRoutes(headerCell(clicked), []),
        role: (role: string) =>
          role === "menuitem" ? collection(labels.map((label) => menuItem(label, clicks))) : node(),
      });
      await (new WebNewDriver(stub.page)[method] as (column: string, direction?: string) => Promise<void>)(...args);
      expect(clicks).toEqual([picked]);
    }
  });

  it("reads the marker from its direction glyph", async () => {
    for (const [cls, expected] of [
      ["lucide lucide-arrow-down-wide-narrow", "ascending"],
      ["lucide lucide-arrow-up-narrow-wide", "descending"],
    ] as const) {
      const header = node({
        locator: (selector: string) =>
          selector === "div.rounded-full svg"
            ? node({ getAttribute: async () => cls })
            : node({ count: async () => 0 }),
      });
      const stub = pageOf(headerRoutes(header, []));
      await expect(new WebNewDriver(stub.page).layoutsSheetSortMarker("State")).resolves.toBe(expected);
    }
    const bare = pageOf(headerRoutes(node({ locator: () => node({ count: async () => 0 }) }), []));
    await expect(new WebNewDriver(bare.page).layoutsSheetSortMarker("State")).resolves.toBe("none");
  });

  it("throws on an unknown marker and a missing class", async () => {
    for (const cls of ["lucide lucide-chevrons-up-down", null]) {
      const header = node({
        locator: (selector: string) =>
          selector === "div.rounded-full svg"
            ? node({ getAttribute: async () => cls })
            : node({ count: async () => 0 }),
      });
      const stub = pageOf(headerRoutes(header, []));
      await expect(new WebNewDriver(stub.page).layoutsSheetSortMarker("State")).rejects.toThrow(
        "[parity] unknown sort marker classes"
      );
    }
  });
});

describe("web_new sheet quick-add", () => {
  it("fills the title, submits, and settles on the new row", async () => {
    const filled: string[] = [];
    const pressed: string[] = [];
    const field = node({
      fill: async (value: string) => void filled.push(value),
      press: async (key: string) => void pressed.push(key),
    });
    const stub = pageOf({
      ...sheetRoutes(sheetTable(["Parity fresh"])),
      role: (role: string) => (role === "button" ? collection([node()]) : node()),
      placeholder: (text: string) => (text === "Work item title" ? field : node()),
    });
    await new WebNewDriver(stub.page).layoutsSheetQuickAdd("Parity fresh");
    expect(filled).toEqual(["Parity fresh"]);
    expect(pressed).toEqual(["Enter"]);
    expect(stub.roles).toContainEqual({ role: "button", name: "Add work item" });
  });

  it("throws when the new row never renders", async () => {
    // Mocked time starts at 1s and each wait advances it past the
    // driver's budget, so the loop throws instead of spinning.
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const stub = pageOf({
        ...sheetRoutes(sheetTable(["Parity other"])),
        role: () => collection([node()]),
        placeholder: () => node(),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsSheetQuickAdd("Parity fresh")).rejects.toThrow(
        "[parity] sheet quick-add never rendered its row."
      );
    } finally {
      spy.mockRestore();
    }
  });
});

describe("web_new sheet sub-issues", () => {
  /** A row: first cell reading `name`, toggle per `toggle`, depth `depth`. */
  function sheetRow(name: string, toggle: "expand" | "collapse" | null, depth: string | null): Locator {
    const cell = firstCell(name);
    return node({
      getAttribute: async (attr: string) => (attr === "data-depth" ? depth : null),
      getByRole: (role: string, options?: unknown) => {
        const wanted = (options as { name?: unknown } | undefined)?.name;
        if (role !== "button") return node({ count: async () => 0 });
        if (toggle === "expand" && wanted === "Expand sub-work items") return node();
        if (toggle === "collapse" && wanted === "Collapse sub-work items") return node();
        return node({ count: async () => 0 });
      },
      locator: (selector: string) => {
        if (selector === 'td[id^="issue-"]') return cell;
        if (selector.startsWith("button[aria-label")) {
          return toggle === null ? node({ count: async () => 0 }) : node();
        }
        return node({ count: async () => 0 });
      },
    });
  }

  it("throws when no row carries the name", async () => {
    const stub = pageOf(sheetRoutes(sheetTable(["Alpha"])));
    await expect(new WebNewDriver(stub.page).layoutsSheetHasSubIssueToggle("Beta")).rejects.toThrow(
      '[parity] no sheet row "Beta".'
    );
    await expect(new WebNewDriver(stub.page).layoutsSheetSubIssueNames("Beta")).rejects.toThrow(
      '[parity] no sheet row "Beta".'
    );
  });

  it("reads the toggle through either label", async () => {
    for (const toggle of ["expand", "collapse", null] as const) {
      const row = sheetRow("Alpha", toggle, "0");
      // layoutsSheetRow scans first-cells; the toggle read needs the row
      // behind the cell, so the ancestor lookup resolves at the row.
      const table = node({
        locator: (selector: string) => {
          if (selector === 'tbody td[id^="issue-"]') {
            return collection([
              node({
                locator: (inner: string) => {
                  if (inner === "a") return node({ innerText: async () => "Alpha" });
                  if (inner === "xpath=ancestor::tr[1]") return row;
                  return node({ count: async () => 0 });
                },
              }),
            ]);
          }
          return node({ count: async () => 0 });
        },
      });
      const stub = pageOf(sheetRoutes(table));
      await expect(new WebNewDriver(stub.page).layoutsSheetHasSubIssueToggle("Alpha")).resolves.toBe(toggle !== null);
    }
  });

  it("toggles without settling", async () => {
    let clicks = 0;
    const row = node({
      locator: (selector: string) =>
        selector.startsWith("button[aria-label")
          ? node({ click: async () => void (clicks += 1) })
          : node({ count: async () => 0 }),
    });
    const table = node({
      locator: (selector: string) => {
        if (selector === 'tbody td[id^="issue-"]') {
          return collection([
            node({
              locator: (inner: string) => {
                if (inner === "a") return node({ innerText: async () => "Alpha" });
                if (inner === "xpath=ancestor::tr[1]") return row;
                return node({ count: async () => 0 });
              },
            }),
          ]);
        }
        return node({ count: async () => 0 });
      },
    });
    const stub = pageOf(sheetRoutes(table));
    await new WebNewDriver(stub.page).layoutsSheetToggleSubIssues("Alpha");
    expect(clicks).toBe(1);
  });

  it("expands and waits for the new rows", async () => {
    let clicks = 0;
    const row = node({
      getByRole: () => node({ click: async () => void (clicks += 1) }),
    });
    const cellFor = (name: string): Locator =>
      node({
        locator: (inner: string) => {
          if (inner === "a") return node({ innerText: async () => name });
          if (inner === "xpath=ancestor::tr[1]") return row;
          return node({ count: async () => 0 });
        },
      });
    // The driver holds one cells locator across the click; its count
    // grows once the children render.
    let grown = false;
    const cells = node({
      count: async () => {
        const current = grown ? 2 : 1;
        grown = true;
        return current;
      },
      nth: (index: number) => cellFor(index === 0 ? "Alpha" : "Beta"),
    });
    const table = node({
      locator: (selector: string) => (selector === 'tbody td[id^="issue-"]' ? cells : node({ count: async () => 0 })),
    });
    const stub = pageOf(sheetRoutes(table));
    await new WebNewDriver(stub.page).layoutsSheetExpandSubIssues("Alpha");
    expect(clicks).toBe(1);
  });

  it("throws when expansion renders nothing new", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const table = node({
        locator: (selector: string) => {
          if (selector === 'tbody td[id^="issue-"]') return collection([firstCell("Alpha")]);
          return node({ count: async () => 0 });
        },
      });
      const stub = pageOf({
        ...sheetRoutes(table),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsSheetExpandSubIssues("Alpha")).rejects.toThrow(
        '[parity] sub-issues of "Alpha" never rendered in the sheet.'
      );
    } finally {
      spy.mockRestore();
    }
  });

  it("collects children while rows stay deeper", async () => {
    const rows = [
      sheetRow("Weird", null, "x"),
      sheetRow("Parent", "expand", "0"),
      sheetRow("Child one", null, "1"),
      sheetRow("Child two", null, "1"),
      sheetRow("Uncle", null, null),
    ];
    const table = node({
      locator: (selector: string) => {
        if (selector === "tbody tr") return collection(rows);
        return node({ count: async () => 0 });
      },
    });
    const stub = pageOf(sheetRoutes(table));
    await expect(new WebNewDriver(stub.page).layoutsSheetSubIssueNames("Parent")).resolves.toEqual([
      "Child one",
      "Child two",
    ]);
  });

  it("reads nothing from an empty table", async () => {
    const stub = pageOf(sheetRoutes(sheetTable([])));
    await expect(new WebNewDriver(stub.page).layoutsSheetSubIssueNames("Parent")).rejects.toThrow(
      '[parity] no sheet row "Parent".'
    );
  });
});

describe("web_new sheet due dates", () => {
  it("sets the month, year, and exact day", async () => {
    const selected: unknown[] = [];
    const clicked: string[] = [];
    const combo = node({ selectOption: async (option: unknown) => void selected.push(option) });
    const stub = pageOf({
      locator: (selector: string) => {
        if (selector === 'td[aria-label="Due date for Alpha"]') return node();
        if (selector === 'button[data-day="2027-11-20"]') {
          return node({ click: async () => void clicked.push(selector) });
        }
        return node({ count: async () => 0 });
      },
      role: (role: string) => (role === "combobox" ? node({ first: () => combo }) : node()),
    });
    await new WebNewDriver(stub.page).layoutsSheetCellSetDueDate("Alpha", "2027-11-20");
    expect(selected).toEqual([{ label: "November" }, { label: "2027" }]);
    expect(clicked).toEqual(['button[data-day="2027-11-20"]']);
  });

  it("falls back on malformed dates", async () => {
    const selected: unknown[] = [];
    const combo = node({ selectOption: async (option: unknown) => void selected.push(option) });
    const stub = pageOf({
      locator: (selector: string) => {
        if (selector === 'td[aria-label="Due date for Alpha"]') return node();
        return node();
      },
      role: (role: string) => (role === "combobox" ? node({ first: () => combo }) : node()),
    });
    await new WebNewDriver(stub.page).layoutsSheetCellSetDueDate("Alpha", "2027-13-01");
    expect(selected[0]).toEqual({ label: "" });
    await new WebNewDriver(stub.page).layoutsSheetCellSetDueDate("Alpha", "2027");
    expect(selected[2]).toEqual({ label: "January" });
  });
});

describe("web_new sheet count and reload", () => {
  it("opens the count and settles on navigation", async () => {
    const cell = node({
      getByRole: (role: string) => (role === "button" ? node({ first: () => node() }) : node()),
    });
    const stub = pageOf({ locator: () => cell });
    await new WebNewDriver(stub.page).layoutsSheetOpenSubIssueCount("Alpha");
    expect(stub.selectors).toEqual(['td[aria-label="Sub-work item for Alpha"]']);
    expect(stub.urlPredicates).toHaveLength(1);
    const settled = stub.urlPredicates[0] ?? (() => false);
    expect(settled(new URL("https://app.test/ws/issues"))).toBe(false);
    expect(settled(new URL("https://app.test/ws/browse/PAR-1#sub-issues"))).toBe(true);
  });

  it("settles reloads on any board layout", async () => {
    const stub = pageOf({ testId: () => node({ count: async () => 0 }) });
    await new WebNewDriver(stub.page).layoutsReloadIssues();
    expect(stub.selectors[0]).toContain("spreadsheet-scroller");
    expect(stub.selectors[0]).toContain("kanban-layout");
    expect(stub.selectors[0]).toContain("gantt-layout");
  });
});
