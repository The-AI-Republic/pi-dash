// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver list-layout methods (NEWFRONT-658,
// ISS-008–011/013–014). A stub Page records the selectors each method
// reads, so dropping the markup hooks the list scenarios depend on fails
// here before the parity stack.
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
  getByLabel?: (text: string) => Locator;
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
    getByLabel: () => node(),
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

/** A group header stub titled `title` with the given expanded state. */
function headerNode(title: string, expanded: boolean, onToggle?: () => void, frozen: boolean = false): Locator {
  let state = expanded;
  const head = node({
    getAttribute: async (name: string) => (name === "aria-expanded" ? String(state) : null),
    locator: (selector: string) => {
      if (selector === "span") {
        return node({
          innerText: async () => title,
          click: async () => {
            if (!frozen) state = !state;
            onToggle?.();
          },
        });
      }
      return node({ count: async () => 0 });
    },
  });
  return head;
}

describe("web-new list groups", () => {
  it("reads expansion from the header's aria-expanded", async () => {
    const headers = collection([headerNode("Todo", true), headerNode("Doing", false)]);
    const stub = pageOf({ locator: () => headers });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.layoutsListGroupExpanded("Todo")).resolves.toBe(true);
    await expect(driver.layoutsListGroupExpanded("Doing")).resolves.toBe(false);
  });

  it("throws for an unknown group", async () => {
    const stub = pageOf({ locator: () => collection([headerNode("Todo", true)]) });
    await expect(new WebNewDriver(stub.page).layoutsListGroupExpanded("Missing")).rejects.toThrow(
      '[parity] no list group "Missing".'
    );
  });

  it("toggles through the title span and settles on the flip", async () => {
    let toggled = 0;
    const head = headerNode("Todo", true, () => void (toggled += 1));
    const stub = pageOf({ locator: () => collection([head]) });
    await new WebNewDriver(stub.page).layoutsListToggleGroup("Todo");
    expect(toggled).toBe(1);
    await expect(new WebNewDriver(stub.page).layoutsListGroupExpanded("Todo")).resolves.toBe(false);
  });

  it("throws when the group never toggles", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const stub = pageOf({
        locator: () => collection([headerNode("Todo", true, undefined, true)]),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsListToggleGroup("Todo")).rejects.toThrow(
        '[parity] list group "Todo" never toggled.'
      );
    } finally {
      spy.mockRestore();
    }
  });
});

/** One section row stub reading its name from the title paragraph. */
function sectionRowNode(name: string): Locator {
  const paragraph = node({ getAttribute: async (attr: string) => (attr === "title" ? name : null) });
  return node({
    locator: (selector: string) => (selector === "p[title]" ? paragraph : node({ count: async () => 0 })),
  });
}

interface SectionHooks {
  names: string[];
  loadMore?: boolean;
  loading?: boolean;
  onLoadMore?: () => void;
}

/** A section stub with rows, an optional Load-more button and skeleton. */
function listSectionNode(hooks: SectionHooks): Locator {
  const rows = (): Locator => collection(hooks.names.map(sectionRowNode));
  const loadMore =
    hooks.loadMore === true
      ? node({
          click: async () => {
            hooks.onLoadMore?.();
          },
        })
      : node({ count: async () => 0 });
  const loading = hooks.loading === true ? node() : node({ count: async () => 0 });
  return node({
    locator: (selector: string) => (selector === '[data-testid="list-row"]' ? rows() : node()),
    getByRole: (role: string, options?: unknown) => {
      if (role === "button" && (options as { name?: unknown } | undefined)?.name === "Load more") return loadMore;
      return node({ count: async () => 0 });
    },
    getByTestId: (id: string) => (id === "section-loading" ? loading : node({ count: async () => 0 })),
  });
}

describe("web-new list paging", () => {
  it("reads the Load-more button when present", async () => {
    const stub = pageOf({ locator: () => listSectionNode({ names: ["Alpha"], loadMore: true }) });
    await expect(new WebNewDriver(stub.page).layoutsListGroupHasLoadMore("Todo")).resolves.toBe(true);
    expect(stub.selectors).toContain("section");
  });

  it("reads false once the section settles with no button", async () => {
    const stub = pageOf({ locator: () => listSectionNode({ names: ["Alpha"] }) });
    await expect(new WebNewDriver(stub.page).layoutsListGroupHasLoadMore("Todo")).resolves.toBe(false);
  });

  it("settles through the loading skeleton", async () => {
    // First poll: fetching (button gone, skeleton up). Second: settled
    // with the button back.
    let buttonCalls = 0;
    let loadingCalls = 0;
    const section = node({
      getByRole: () => node({ count: async () => (buttonCalls++ === 0 ? 0 : 1) }),
      getByTestId: () => node({ count: async () => (loadingCalls++ === 0 ? 1 : 0) }),
    });
    const stub = pageOf({ locator: () => section });
    await expect(new WebNewDriver(stub.page).layoutsListGroupHasLoadMore("Todo")).resolves.toBe(true);
  });

  it("throws when paging never settles", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const stub = pageOf({
        locator: () => listSectionNode({ names: ["Alpha"], loading: true }),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsListGroupHasLoadMore("Todo")).rejects.toThrow(
        '[parity] list group "Todo" never settled its paging.'
      );
    } finally {
      spy.mockRestore();
    }
  });

  it("loads more and settles on the appended rows", async () => {
    const hooks: SectionHooks = {
      names: ["Alpha"],
      loadMore: true,
      onLoadMore: () => {
        hooks.names.push("Beta");
        hooks.loadMore = false;
      },
    };
    const stub = pageOf({ locator: () => listSectionNode(hooks) });
    await new WebNewDriver(stub.page).layoutsListGroupLoadMore("Todo");
    expect(hooks.names).toEqual(["Alpha", "Beta"]);
  });

  it("throws when the next page never renders", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const stub = pageOf({
        locator: () => listSectionNode({ names: ["Alpha"], loadMore: true }),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsListGroupLoadMore("Todo")).rejects.toThrow(
        '[parity] list group "Todo" never loaded its next page.'
      );
    } finally {
      spy.mockRestore();
    }
  });

  it("scrolls the Issues region to its end", async () => {
    let scrolled = 0;
    const scroller = node({
      evaluate: async () => void (scrolled += 1),
    });
    const stub = pageOf({ role: (role: string) => (role === "region" ? scroller : node({ count: async () => 0 })) });
    await new WebNewDriver(stub.page).layoutsListScrollEnd();
    expect(scrolled).toBe(1);
    expect(stub.roles).toEqual([{ role: "region", name: "Issues" }]);
  });
});

describe("web-new list quick-add", () => {
  it("creates through the flat form and settles on the row", async () => {
    const filled: string[] = [];
    const pressed: string[] = [];
    let opened = 0;
    const input = node({
      fill: async (value: string) => void filled.push(value),
      press: async (key: string) => void pressed.push(key),
    });
    const form = node({
      getByRole: (role: string, options?: unknown) => {
        if (role === "button" && (options as { name?: unknown } | undefined)?.name === "New work item") {
          return node({
            click: async () => void (opened += 1),
          });
        }
        return node({ count: async () => 0 });
      },
      getByLabel: (text: string) => (text === "New work item title" ? input : node({ count: async () => 0 })),
    });
    const stub = pageOf({
      locator: () => listSectionNode({ names: ["Parity quickadd x"] }),
      testId: (id: string) => (id === "quick-add" ? form : node({ count: async () => 0 })),
    });
    await new WebNewDriver(stub.page).layoutsListQuickAdd("Parity quickadd x");
    expect(opened).toBe(1);
    expect(filled).toEqual(["Parity quickadd x"]);
    expect(pressed).toEqual(["Enter"]);
  });

  it("scopes the form to the named section when grouped", async () => {
    const filled: string[] = [];
    const input = node({ fill: async (value: string) => void filled.push(value) });
    const form = node({
      getByRole: () => node(),
      getByLabel: () => input,
    });
    const grouped = node({
      locator: (selector: string) =>
        selector === '[data-testid="list-row"]'
          ? collection(["Parity sectionadd x"].map(sectionRowNode))
          : node({ count: async () => 0 }),
      getByTestId: (id: string) => (id === "quick-add" ? form : node({ count: async () => 0 })),
    });
    const stub = pageOf({ locator: () => grouped });
    await new WebNewDriver(stub.page).layoutsListQuickAdd("Parity sectionadd x", "Doing");
    expect(filled).toEqual(["Parity sectionadd x"]);
    expect(stub.selectors).toContain("section");
  });

  it("throws when the created row never renders", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const form = node({ getByRole: () => node(), getByLabel: () => node() });
      const stub = pageOf({
        locator: () => listSectionNode({ names: ["Alpha"] }),
        testId: (id: string) => (id === "quick-add" ? form : node({ count: async () => 0 })),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsListQuickAdd("Parity missing")).rejects.toThrow(
        '[parity] quick-add never rendered "Parity missing".'
      );
    } finally {
      spy.mockRestore();
    }
  });
});

interface RowHooks {
  stateTitle?: string;
  priorityTitle?: string;
  chipDisabled?: boolean;
  expand?: boolean;
  collapse?: boolean;
  peekOpener?: boolean;
  children?: string[];
  onExpand?: () => void;
  onChipClick?: () => void;
  titles?: () => string;
}

/** One child article stub headed by its own titled row. */
function childArticleNode(name: string): Locator {
  const paragraph = node({ getAttribute: async (attr: string) => (attr === "title" ? name : null) });
  const head = node({
    locator: (selector: string) => (selector === "p[title]" ? paragraph : node({ count: async () => 0 })),
  });
  return node({
    locator: (selector: string) => (selector === '[data-testid="list-row"]' ? head : node({ count: async () => 0 })),
  });
}

/** A list-row stub with chip wrappers, toggle buttons and nested children. */
function listRowNode(issueName: string, hooks: RowHooks): Locator {
  const stateWrap = node({
    getAttribute: async (attr: string) => {
      if (attr !== "title") return null;
      return hooks.titles === undefined ? (hooks.stateTitle ?? null) : hooks.titles();
    },
    locator: (selector: string) =>
      selector === "button"
        ? node({
            getAttribute: async (attr: string) => (attr === "disabled" && hooks.chipDisabled === true ? "" : null),
            click: async () => {
              hooks.onChipClick?.();
            },
          })
        : node({ count: async () => 0 }),
  });
  const priorityWrap = node({
    getAttribute: async (attr: string) => (attr === "title" ? (hooks.priorityTitle ?? null) : null),
    locator: (selector: string) =>
      selector === "button"
        ? node({
            click: async () => {
              hooks.onChipClick?.();
            },
          })
        : node({ count: async () => 0 }),
  });
  const buttons = new Map<string, Locator>([
    [
      `Expand sub-work items of ${issueName}`,
      hooks.expand === true
        ? node({
            click: async () => {
              hooks.onExpand?.();
            },
          })
        : node({ count: async () => 0 }),
    ],
    [`Collapse sub-work items of ${issueName}`, hooks.collapse === true ? node() : node({ count: async () => 0 })],
  ]);
  const container =
    hooks.children === undefined
      ? node({ count: async () => 0 })
      : node({
          locator: (selector: string) =>
            selector === "xpath=child::article"
              ? collection((hooks.children ?? []).map(childArticleNode))
              : node({ count: async () => 0 }),
        });
  return node({
    locator: (selector: string) => {
      if (selector === 'span[title^="State: "]') return stateWrap;
      if (selector === 'span[title^="Priority: "]') return priorityWrap;
      if (selector.startsWith("xpath=")) return container;
      return node({ count: async () => 0 });
    },
    getByRole: (role: string, options?: unknown) => {
      if (role !== "button") return node({ count: async () => 0 });
      const name = (options as { name?: unknown } | undefined)?.name;
      if (typeof name === "string" && buttons.has(name)) return buttons.get(name) ?? node();
      if (hooks.peekOpener === true && name === `Open sub-work items of ${issueName} in preview`) return node();
      return node({ count: async () => 0 });
    },
  });
}

describe("web-new row chips", () => {
  it("reads the state name from the chip title", async () => {
    const stub = pageOf({ locator: () => listRowNode("Alpha", { stateTitle: "State: Todo" }) });
    await expect(new WebNewDriver(stub.page).layoutsRowState("Alpha")).resolves.toBe("Todo");
  });

  it("capitalizes the raw priority level", async () => {
    const high = pageOf({ locator: () => listRowNode("Alpha", { priorityTitle: "Priority: high" }) });
    await expect(new WebNewDriver(high.page).layoutsRowPriority("Alpha")).resolves.toBe("High");
    const none = pageOf({ locator: () => listRowNode("Alpha", { priorityTitle: "Priority: none" }) });
    await expect(new WebNewDriver(none.page).layoutsRowPriority("Alpha")).resolves.toBe("None");
  });

  it("reads editability from the state chip", async () => {
    let waited = 0;
    const chip = node({
      waitFor: async () => void (waited += 1),
      getAttribute: async () => null,
    });
    const wrap = node({ locator: () => chip });
    const row = node({
      locator: (selector: string) => (selector === 'span[title^="State: "]' ? wrap : node({ count: async () => 0 })),
    });
    const enabled = pageOf({ locator: () => row });
    await expect(new WebNewDriver(enabled.page).layoutsRowCanEditState("Alpha")).resolves.toBe(true);
    expect(waited).toBe(1);
    const noChipRow = node({
      locator: (selector: string) =>
        selector === 'span[title^="State: "]'
          ? node({
              locator: () =>
                node({
                  waitFor: async () => {
                    throw new Error("Timeout");
                  },
                }),
            })
          : node({ count: async () => 0 }),
    });
    const missing = pageOf({ locator: () => noChipRow });
    await expect(new WebNewDriver(missing.page).layoutsRowCanEditState("Alpha")).resolves.toBe(false);
  });

  it("reads a disabled chip as cannot-edit", async () => {
    const disabled = pageOf({
      locator: () => listRowNode("Alpha", { stateTitle: "State: Todo", chipDisabled: true }),
    });
    await expect(new WebNewDriver(disabled.page).layoutsRowCanEditState("Alpha")).resolves.toBe(false);
  });

  it("sets state through the combobox and settles on the repaint", async () => {
    let current = "State: Todo";
    const filled: string[] = [];
    const picked: unknown[] = [];
    let chipClicks = 0;
    const option = node({
      click: async () => {
        current = "State: Doing";
      },
    });
    const listbox = node({
      getByRole: (role: string, options?: unknown) => {
        if (role === "option") picked.push((options as { name?: unknown } | undefined)?.name);
        return option;
      },
    });
    const search = node({ fill: async (value: string) => void filled.push(value) });
    const stub = pageOf({
      locator: () =>
        listRowNode("Alpha", {
          titles: () => current,
          onChipClick: () => void (chipClicks += 1),
        }),
      role: (role: string) => {
        if (role === "listbox") return listbox;
        if (role === "combobox") return search;
        return node({ count: async () => 0 });
      },
    });
    await new WebNewDriver(stub.page).layoutsRowSetState("Alpha", "Doing");
    expect(chipClicks).toBe(1);
    expect(filled).toEqual(["Doing"]);
    expect(picked).toEqual(["Doing"]);
    expect(stub.roles).toContainEqual({ role: "combobox", name: "Search states" });
  });

  it("sets priority through its own search and lowercases the settle", async () => {
    let current = "Priority: none";
    const option = node({
      click: async () => {
        current = "Priority: high";
      },
    });
    const listbox = node({ getByRole: () => option });
    const priorityRow = node({
      locator: (selector: string) => {
        if (selector === 'span[title^="Priority: "]') {
          return node({
            getAttribute: async (attr: string) => (attr === "title" ? current : null),
            locator: () => node(),
          });
        }
        return node({ count: async () => 0 });
      },
    });
    const priorityStub = pageOf({
      locator: () => priorityRow,
      role: (role: string) => {
        if (role === "listbox") return listbox;
        if (role === "combobox") return node();
        return node({ count: async () => 0 });
      },
    });
    await new WebNewDriver(priorityStub.page).layoutsRowSetPriority("Alpha", "High");
    expect(priorityStub.roles).toContainEqual({ role: "combobox", name: "Search priorities" });
  });

  it("throws when the chip never repaints", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const row = node({
        locator: (selector: string) => {
          if (selector === 'span[title^="State: "]') {
            return node({
              getAttribute: async (attr: string) => (attr === "title" ? "State: Todo" : null),
              locator: () => node(),
            });
          }
          return node({ count: async () => 0 });
        },
      });
      const listbox = node({ getByRole: () => node() });
      const stub = pageOf({
        locator: () => row,
        role: (role: string) => {
          if (role === "listbox") return listbox;
          if (role === "combobox") return node();
          return node({ count: async () => 0 });
        },
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsRowSetState("Alpha", "Doing")).rejects.toThrow(
        '[parity] row chip for "Alpha" never showed "Doing".'
      );
    } finally {
      spy.mockRestore();
    }
  });
});

describe("web-new row sub-issues", () => {
  it("detects the expander in either state, but not the peek opener", async () => {
    const expand = pageOf({ locator: () => listRowNode("Alpha", { expand: true }) });
    await expect(new WebNewDriver(expand.page).layoutsRowHasSubIssueToggle("Alpha")).resolves.toBe(true);
    const collapse = pageOf({ locator: () => listRowNode("Alpha", { collapse: true }) });
    await expect(new WebNewDriver(collapse.page).layoutsRowHasSubIssueToggle("Alpha")).resolves.toBe(true);
    const leaf = pageOf({ locator: () => listRowNode("Alpha", {}) });
    await expect(new WebNewDriver(leaf.page).layoutsRowHasSubIssueToggle("Alpha")).resolves.toBe(false);
    // The at-cap peek opener must not count: the read queries only the
    // expand/collapse names, never the preview variant.
    const queried: unknown[] = [];
    const cappedRow = node({
      getByRole: (_role: string, options?: unknown) => {
        queried.push((options as { name?: unknown } | undefined)?.name);
        return node({ count: async () => 0 });
      },
    });
    const capped = pageOf({ locator: () => cappedRow });
    await expect(new WebNewDriver(capped.page).layoutsRowHasSubIssueToggle("Alpha")).resolves.toBe(false);
    expect(queried).toEqual(["Expand sub-work items of Alpha", "Collapse sub-work items of Alpha"]);
  });

  it("expands and settles on the rendered children", async () => {
    let expanded = 0;
    const stub = pageOf({
      locator: () =>
        listRowNode("Alpha", {
          expand: true,
          children: ["Beta"],
          onExpand: () => void (expanded += 1),
        }),
    });
    await new WebNewDriver(stub.page).layoutsRowExpandSubIssues("Alpha");
    expect(expanded).toBe(1);
  });

  it("skips the click when already expanded", async () => {
    let expanded = 0;
    const stub = pageOf({
      locator: () =>
        listRowNode("Alpha", {
          collapse: true,
          children: ["Beta"],
          onExpand: () => void (expanded += 1),
        }),
    });
    await new WebNewDriver(stub.page).layoutsRowExpandSubIssues("Alpha");
    expect(expanded).toBe(0);
  });

  it("throws when expanded children never render", async () => {
    let now = 1_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => now);
    try {
      const stub = pageOf({
        locator: () => listRowNode("Alpha", { expand: true, children: [] }),
        tick: () => {
          now += 60_000;
        },
      });
      await expect(new WebNewDriver(stub.page).layoutsRowExpandSubIssues("Alpha")).rejects.toThrow(
        '[parity] sub-issues of "Alpha" never rendered in the list.'
      );
    } finally {
      spy.mockRestore();
    }
  });

  it("reads direct children and skips empty names", async () => {
    const stub = pageOf({ locator: () => listRowNode("Alpha", { children: ["Beta", "", "Gamma"] }) });
    await expect(new WebNewDriver(stub.page).layoutsRowSubIssueNames("Alpha")).resolves.toEqual(["Beta", "Gamma"]);
  });

  it("reads no children when unexpanded", async () => {
    const stub = pageOf({ locator: () => listRowNode("Alpha", {}) });
    await expect(new WebNewDriver(stub.page).layoutsRowSubIssueNames("Alpha")).resolves.toEqual([]);
  });
});
