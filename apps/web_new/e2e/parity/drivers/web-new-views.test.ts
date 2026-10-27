// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver views-area methods (NEWFRONT-745,
// VIEW-001..048). A stub Page stands in for Playwright: every query the
// driver makes is recorded, and the resolver returns a locator shaped per
// test, so a markup change that drops a parity hook fails here before the
// parity stack is brought up.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

type Stub = Record<string, unknown> & {
  count?: ReturnType<typeof vi.fn>;
  isVisible?: ReturnType<typeof vi.fn>;
  isEnabled?: ReturnType<typeof vi.fn>;
  innerText?: ReturnType<typeof vi.fn>;
  allInnerTexts?: ReturnType<typeof vi.fn>;
  click?: ReturnType<typeof vi.fn>;
  fill?: ReturnType<typeof vi.fn>;
  waitFor?: ReturnType<typeof vi.fn>;
  focus?: ReturnType<typeof vi.fn>;
  press?: ReturnType<typeof vi.fn>;
  inputValue?: ReturnType<typeof vi.fn>;
  getAttribute?: ReturnType<typeof vi.fn>;
  evaluate?: ReturnType<typeof vi.fn>;
  evaluateAll?: ReturnType<typeof vi.fn>;
  dragTo?: ReturnType<typeof vi.fn>;
};

interface Query {
  kind: "testid" | "role" | "text" | "placeholder" | "selector";
  value: string;
  options?: Record<string, unknown> | undefined;
}

function loc(overrides: Stub = {}): Locator {
  const node: Stub = {
    count: vi.fn(async () => 1),
    isVisible: vi.fn(async () => true),
    isEnabled: vi.fn(async () => true),
    innerText: vi.fn(async () => ""),
    allInnerTexts: vi.fn(async () => []),
    click: vi.fn(async () => undefined),
    fill: vi.fn(async () => undefined),
    waitFor: vi.fn(async () => undefined),
    focus: vi.fn(async () => undefined),
    press: vi.fn(async () => undefined),
    inputValue: vi.fn(async () => ""),
    getAttribute: vi.fn(async () => null),
    evaluate: vi.fn(async (fn: (node: unknown) => unknown) => fn(node)),
    evaluateAll: vi.fn(async (fn: (nodes: unknown[]) => unknown) => fn([])),
    dragTo: vi.fn(async () => undefined),
    all: vi.fn(async () => []),
    ...overrides,
  };
  for (const name of [
    "locator",
    "getByRole",
    "getByTestId",
    "getByText",
    "getByPlaceholder",
    "filter",
    "or",
  ] as const) {
    if (node[name] === undefined) node[name] = vi.fn(() => node);
  }
  node.first ??= () => node;
  node.last ??= () => node;
  node.nth ??= vi.fn(() => node);
  return node as unknown as Locator;
}

const ABSENT = (): Locator => loc({ count: vi.fn(async () => 0), isVisible: vi.fn(async () => false) });

interface Harness {
  page: Page;
  queries: Query[];
  /** The node every query resolves to unless `answer` names another. */
  node: Locator;
  route: ReturnType<typeof vi.fn>;
  unroute: ReturnType<typeof vi.fn>;
  press: ReturnType<typeof vi.fn>;
  mouseClick: ReturnType<typeof vi.fn>;
  evaluate: ReturnType<typeof vi.fn>;
  goto: ReturnType<typeof vi.fn>;
  waitForEvent: ReturnType<typeof vi.fn>;
  addInitScript: ReturnType<typeof vi.fn>;
  waitForFunction: ReturnType<typeof vi.fn>;
  grantPermissions: ReturnType<typeof vi.fn>;
  title: ReturnType<typeof vi.fn>;
}

/**
 * A page whose every query (top-level or chained off another locator) is
 * recorded and answered by `answer`, falling back to a single shared stub
 * node so chained reads stay scriptable from one place.
 */
function harness(answer?: (query: Query) => Locator | undefined): Harness {
  const queries: Query[] = [];
  const state = {} as Harness;
  let base: Stub;
  const resolve = (query: Query): Locator => {
    queries.push(query);
    const named = answer?.(query);
    if (named !== undefined) return named;
    return base as unknown as Locator;
  };
  base = {
    count: vi.fn(async () => 1),
    isVisible: vi.fn(async () => true),
    isEnabled: vi.fn(async () => true),
    innerText: vi.fn(async () => ""),
    allInnerTexts: vi.fn(async () => []),
    click: vi.fn(async () => undefined),
    fill: vi.fn(async () => undefined),
    waitFor: vi.fn(async () => undefined),
    focus: vi.fn(async () => undefined),
    press: vi.fn(async () => undefined),
    inputValue: vi.fn(async () => ""),
    getAttribute: vi.fn(async () => null),
    evaluate: vi.fn(async (fn: (node: unknown) => unknown) => fn(base)),
    evaluateAll: vi.fn(async (fn: (nodes: unknown[]) => unknown) => fn([])),
    dragTo: vi.fn(async () => undefined),
    all: vi.fn(async () => []),
    locator: vi.fn((value: string, options?: Record<string, unknown>) => resolve({ kind: "selector", value, options })),
    getByRole: vi.fn((value: string, options?: Record<string, unknown>) => resolve({ kind: "role", value, options })),
    getByTestId: vi.fn((value: string) => resolve({ kind: "testid", value })),
    getByText: vi.fn((value: string, options?: Record<string, unknown>) => resolve({ kind: "text", value, options })),
    getByPlaceholder: vi.fn((value: string, options?: Record<string, unknown>) =>
      resolve({ kind: "placeholder", value, options })
    ),
    filter: vi.fn(() => base),
    or: vi.fn(() => base),
    first: () => base,
    last: () => base,
    nth: vi.fn(() => base),
  };
  state.queries = queries;
  state.asks = [];
  Object.defineProperty(state, "asks", {
    get: () => queries.map((query) => `${query.kind}:${query.value}`),
  });
  state.node = base as unknown as Locator;
  state.route = vi.fn(async () => undefined);
  state.unroute = vi.fn(async () => undefined);
  state.press = vi.fn(async () => undefined);
  state.mouseClick = vi.fn(async () => undefined);
  state.evaluate = vi.fn(async (fn: (arg?: unknown) => unknown, arg?: unknown) => fn(arg));
  state.goto = vi.fn(async () => null);
  state.waitForEvent = vi.fn(async () => ({
    url: () => "https://app.test/acme/workspace-views/1",
    close: async () => undefined,
  }));
  state.addInitScript = vi.fn(async () => undefined);
  state.waitForFunction = vi.fn(async () => undefined);
  state.grantPermissions = vi.fn(async () => undefined);
  state.title = vi.fn(async () => "Acme - Views");
  state.page = {
    goto: state.goto,
    title: state.title,
    getByTestId: base.getByTestId,
    getByRole: base.getByRole,
    getByText: base.getByText,
    getByPlaceholder: base.getByPlaceholder,
    locator: base.locator,
    route: state.route,
    unroute: state.unroute,
    addInitScript: state.addInitScript,
    waitForFunction: state.waitForFunction,
    waitForLoadState: vi.fn(async () => undefined),
    waitForEvent: state.waitForEvent,
    evaluate: state.evaluate,
    keyboard: { press: state.press },
    mouse: { click: state.mouseClick },
    context: () => ({ grantPermissions: state.grantPermissions }),
  } as unknown as Page;
  return state;
}

/** Query kinds the test asked for, as "kind:value" strings. */
function asked(state: Harness): string[] {
  return state.queries.map((query) => `${query.kind}:${query.value}`);
}

function askedWith(state: Harness, kind: Query["kind"], value: string) {
  return state.queries.filter((query) => query.kind === kind && query.value === value);
}

describe("viewsText and viewsTexts", () => {
  it("returns an empty string for an absent element", async () => {
    const state = harness((query) => (query.kind === "testid" && query.value === "t" ? ABSENT() : undefined));
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsRowAccess("Alpha")).resolves.toBe("");
  });

  it("normalises whitespace and survives an innerText rejection", async () => {
    const state = harness();
    (state.node as never as Stub).innerText = vi.fn(async () => "  Two\n rows ");
    const rejected = harness();
    (rejected.node as never as Stub).innerText = vi.fn(async () => Promise.reject(new Error("x")));
    await expect(new WebNewDriver(state.page).viewsRowAccess("Alpha")).resolves.toBe("Two rows");
    await expect(new WebNewDriver(rejected.page).viewsRowAccess("Alpha")).resolves.toBe("");
  });

  it("drops empty entries when reading a list of texts", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => [" Created at ", "", "  Due  "]);
    await expect(new WebNewDriver(state.page).viewsChipTexts()).resolves.toEqual(["Created at", "Due"]);
  });

  it("returns no texts when the list read rejects", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => Promise.reject(new Error("x")));
    await expect(new WebNewDriver(state.page).viewsChipTexts()).resolves.toEqual([]);
  });

  it("returns an empty string for a card that is not shown", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-zero" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(state.page).viewsEmptyTitle()).resolves.toBe("");
  });

  it("reads the card heading from the title span", async () => {
    const title = loc({ innerText: vi.fn(async () => "  No views yet  ") });
    const card = loc({ locator: vi.fn(() => title) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-zero" ? card : undefined
    );
    await expect(new WebNewDriver(state.page).viewsEmptyTitle()).resolves.toBe("No views yet");
  });
});

describe("views list navigation and chrome", () => {
  it("opens the list and waits for the screen", async () => {
    const screen = loc();
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-screen" ? screen : undefined
    );
    await new WebNewDriver(state.page).viewsOpenList("acme", "p-1");
    expect(state.goto).toHaveBeenCalledWith("/acme/projects/p-1/views", { waitUntil: "domcontentloaded" });
    expect(screen.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 120_000 });
  });

  it("opens a saved view detail and accepts either the header or the missing card", async () => {
    const header = loc();
    const state = harness((query) =>
      query.kind === "selector" &&
      query.value === '[data-testid="saved-view-header"], [data-testid="saved-view-missing"]'
        ? header
        : undefined
    );
    await new WebNewDriver(state.page).viewsOpenDetail("acme", "p-1", "v-1");
    expect(state.goto).toHaveBeenCalledWith("/acme/projects/p-1/views/v-1", { waitUntil: "domcontentloaded" });
    expect(header.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 120_000 });
  });

  it("reads the list breadcrumb trail", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Acme", "Apollo", "Views"]);
    await expect(new WebNewDriver(state.page).viewsListBreadcrumb()).resolves.toEqual(["Acme", "Apollo", "Views"]);
    expect(askedWith(state, "testid", "project-views-header")).toHaveLength(1);
    expect(askedWith(state, "selector", "ol > li")).toHaveLength(1);
  });

  it("reads the tab title from the page", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).viewsListTabTitle()).resolves.toBe("Acme - Views");
    await expect(new WebNewDriver(state.page).viewsDetailTabTitle()).resolves.toBe("Acme - Views");
    await expect(new WebNewDriver(state.page).wsViewsListTabTitle()).resolves.toBe("Acme - Views");
    await expect(new WebNewDriver(state.page).wsViewsDetailTabTitle()).resolves.toBe("Acme - Views");
  });

  it("reports the header action and the skeleton", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).viewsHeaderAddVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).viewsListSkeletonVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).wsViewsSkeletonVisible()).resolves.toBe(true);
    expect(asked(state)).toContain("role:button");
    expect(asked(state)).toContain("testid:project-views-skeleton");
    const options = askedWith(state, "role", "button")[0]!.options;
    expect(options).toEqual({ name: "Add view", exact: true });
  });

  it("reports an absent header action as hidden", async () => {
    const state = harness(() => ABSENT());
    await expect(new WebNewDriver(state.page).viewsHeaderAddVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(state.page).viewsListSkeletonVisible()).resolves.toBe(false);
  });

  it("holds the next project views read and lets non-GETs straight through", async () => {
    const state = harness();
    const driver = new WebNewDriver(state.page);
    await driver.viewsDelayListLoad(5);
    expect(state.route).toHaveBeenCalledWith(expect.any(Function), expect.any(Function), { times: 1 });
    const [match, handler] = state.route.mock.calls[0] as [(url: URL) => boolean, (route: Route) => Promise<void>];
    expect(match(new URL("https://api.test/api/workspaces/w/projects/p/views/"))).toBe(true);
    expect(match(new URL("https://api.test/api/workspaces/w/projects/p/views/v/"))).toBe(false);
    expect(match(new URL("https://api.test/api/workspaces/w/views/"))).toBe(false);
    const get = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(get as unknown as Route);
    expect(get.continue).toHaveBeenCalled();
    const post = { request: () => ({ method: () => "POST" }), continue: vi.fn(async () => undefined) };
    await handler(post as unknown as Route);
    expect(post.continue).toHaveBeenCalled();
  });
});

describe("views create dialog", () => {
  it("opens the dialog from the list header", async () => {
    const heading = loc();
    const trigger = loc();
    const state = harness((query) => {
      if (query.kind === "role" && query.value === "button") return trigger;
      if (query.kind === "role" && query.value === "heading") return heading;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsOpenCreate();
    expect(trigger.click).toHaveBeenCalled();
    expect(heading.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the layout control value", async () => {
    const state = harness();
    (state.node as never as Stub).innerText = vi.fn(async () => "Board");
    await expect(new WebNewDriver(state.page).viewsLayoutValue()).resolves.toBe("Board");
  });

  it("opens the layout picker only when it is closed", async () => {
    const state = harness((query) =>
      query.kind === "role" && query.value === "listbox" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsLayoutOpenPicker()).resolves.toBeUndefined();
    const control = (state.node as never as Stub).click as ReturnType<typeof vi.fn>;
    expect(control).toHaveBeenCalled();
  });

  it("leaves an already open layout picker alone", async () => {
    const state = harness();
    await new WebNewDriver(state.page).viewsLayoutOpenPicker();
    const click = (state.node as never as Stub).click as ReturnType<typeof vi.fn>;
    expect(click).not.toHaveBeenCalled();
  });

  it("reads the layout option labels", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["List", "Board"]);
    await expect(new WebNewDriver(state.page).viewsLayoutOptionTexts()).resolves.toEqual(["List", "Board"]);
  });

  it("detects a search field inside the picker", async () => {
    const withInput = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 1) })) });
    const withoutInput = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 0) })) });
    const absent = loc({ count: vi.fn(async () => 0) });
    await expect(
      new WebNewDriver(
        harness((q) => (q.kind === "role" && q.value === "listbox" ? withInput : undefined)).page
      ).viewsLayoutHasSearch()
    ).resolves.toBe(true);
    await expect(
      new WebNewDriver(
        harness((q) => (q.kind === "role" && q.value === "listbox" ? withoutInput : undefined)).page
      ).viewsLayoutHasSearch()
    ).resolves.toBe(false);
    await expect(
      new WebNewDriver(
        harness((q) => (q.kind === "role" && q.value === "listbox" ? absent : undefined)).page
      ).viewsLayoutHasSearch()
    ).resolves.toBe(false);
  });

  it("reports the selected layout option", async () => {
    const withAttr = async (selected: string | null) => {
      const state = harness();
      (state.node as never as Stub).getAttribute = vi.fn(async () => selected);
      return new WebNewDriver(state.page).viewsLayoutSelectedMarked("List");
    };
    await expect(withAttr("true")).resolves.toBe(true);
    await expect(withAttr(null)).resolves.toBe(false);
    const state = harness((query) =>
      query.kind === "role" && query.value === "option" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await expect(new WebNewDriver(state.page).viewsLayoutSelectedMarked("List")).resolves.toBe(false);
  });

  it("picks a layout and waits for the picker to close", async () => {
    const option = loc();
    const listbox = loc({ getByRole: vi.fn(() => option) });
    const state = harness((query) => (query.kind === "role" && query.value === "listbox" ? listbox : undefined));
    await new WebNewDriver(state.page).viewsLayoutPick("Board");
    expect(option.click).toHaveBeenCalled();
    expect(listbox.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("fills the title and submits the dialog", async () => {
    const state = harness();
    const node = state.node as never as Stub;
    const driver = new WebNewDriver(state.page);
    await driver.viewsFillName("Sprint board");
    await driver.viewsSubmit();
    expect(node.fill).toHaveBeenCalledWith("Sprint board");
    expect(asked(state)).toContain("placeholder:Title");
    expect(asked(state)).toContain('selector:button[type="submit"]');
    expect(node.click).toHaveBeenCalled();
    const headings = askedWith(state, "role", "heading");
    expect(headings.some((query) => query.options?.["name"] instanceof RegExp)).toBe(true);
  });

  it("reads the icon preview", async () => {
    const state = harness();
    (state.node as never as Stub).innerText = vi.fn(async () => "Layers");
    await expect(new WebNewDriver(state.page).viewsDialogIconPreview()).resolves.toBe("Layers");
    expect(asked(state)).toContain("testid:view-icon-picker");
  });
});

describe("views list rows and search", () => {
  function rowFor(name: string, node: Locator): (query: Query) => Locator | undefined {
    return (query) => {
      if (query.kind === "testid" && query.value === "project-views-row")
        return loc({ filter: vi.fn(() => (name === "Alpha" ? node : ABSENT())) });
      return undefined;
    };
  }

  it("reads a row's href, access badge, owner and live marker", async () => {
    const link = loc({ getAttribute: vi.fn(async () => "/acme/projects/p/views/v") });
    const row = loc({
      locator: vi.fn(() => link),
      getByTestId: vi.fn(() => loc({ innerText: vi.fn(async () => "Public") })),
    });
    const state = harness(rowFor("Alpha", row));
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsRowHref("Alpha")).resolves.toBe("/acme/projects/p/views/v");
    await expect(driver.viewsRowAccess("Alpha")).resolves.toBe("Public");
    await expect(driver.viewsRowOwnerAvatar("Alpha")).resolves.toBe(true);
    await expect(driver.viewsRowLiveVisible("Alpha")).resolves.toBe(true);
    await expect(driver.viewsRowText("Alpha")).resolves.toBe("");
  });

  it("returns null for a missing row", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-row" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(state.page).viewsRowHref("Nope")).resolves.toBeNull();
  });

  it("reads the list row names", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => [" Alpha ", "", "Beta"]);
    await expect(new WebNewDriver(state.page).viewsListNames()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("opens search and types into it", async () => {
    const input = loc();
    const root = loc({ getByRole: vi.fn(() => input), locator: vi.fn(() => loc({ count: vi.fn(async () => 0) })) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-search" ? root : undefined
    );
    const driver = new WebNewDriver(state.page);
    await driver.viewsSearchOpen();
    expect(input.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    await driver.viewsSearchType("beta");
    expect(input.fill).toHaveBeenCalledWith("beta");
  });

  it("focuses an already expanded search box", async () => {
    const input = loc();
    const root = loc({ getByRole: vi.fn(() => input), locator: vi.fn(() => loc()) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-search" ? root : undefined
    );
    await new WebNewDriver(state.page).viewsSearchOpen();
    expect(input.click).toHaveBeenCalled();
    expect(input.waitFor).not.toHaveBeenCalled();
  });

  it("reports expanded, trigger and value", async () => {
    const input = loc({ inputValue: vi.fn(async () => "beta") });
    const root = loc({ getByRole: vi.fn(() => input), locator: vi.fn(() => loc()) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-search" ? root : undefined
    );
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsSearchExpanded()).resolves.toBe(true);
    await expect(driver.viewsSearchTriggerVisible()).resolves.toBe(true);
    await expect(driver.viewsSearchValue()).resolves.toBe("beta");
    const collapsed = harness((query) =>
      query.kind === "testid" && query.value === "project-views-search"
        ? loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 0) })) })
        : undefined
    );
    await expect(new WebNewDriver(collapsed.page).viewsSearchExpanded()).resolves.toBe(false);
  });

  it("returns an empty value when the search box is not rendered", async () => {
    const input = loc({ count: vi.fn(async () => 0) });
    const root = loc({ getByRole: vi.fn(() => input) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-search" ? root : undefined
    );
    await expect(new WebNewDriver(state.page).viewsSearchValue()).resolves.toBe("");
  });

  it("reports focus from the active element", async () => {
    const focused = loc({ evaluate: vi.fn(async () => true) });
    const blurred = loc({ evaluate: vi.fn(async () => false) });
    const absent = loc({ count: vi.fn(async () => 0) });
    const withInput = (input: Locator) =>
      harness((query) =>
        query.kind === "testid" && query.value === "project-views-search"
          ? loc({ getByRole: vi.fn(() => input) })
          : undefined
      ).page;
    await expect(new WebNewDriver(withInput(focused)).viewsSearchFocused()).resolves.toBe(true);
    await expect(new WebNewDriver(withInput(blurred)).viewsSearchFocused()).resolves.toBe(false);
    await expect(new WebNewDriver(withInput(absent)).viewsSearchFocused()).resolves.toBe(false);
  });

  it("presses Escape, clears and clicks outside", async () => {
    const input = loc();
    const root = loc({ getByRole: vi.fn(() => input) });
    const trail = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-search") return root;
      if (query.kind === "testid" && query.value === "project-views-header")
        return loc({ getByRole: vi.fn(() => trail) });
      return undefined;
    });
    const driver = new WebNewDriver(state.page);
    await driver.viewsSearchEscape();
    await driver.viewsSearchClear();
    await driver.viewsSearchClickOutside();
    expect(input.press).toHaveBeenCalledWith("Escape");
    expect(root.getByRole).toHaveBeenCalledWith("button", { name: "Clear search", exact: true });
    expect(trail.click).toHaveBeenCalledWith({ position: { x: 2, y: 2 }, timeout: 30_000 });
  });
});

describe("views sort menu", () => {
  it("reads the trigger text and opens the menu", async () => {
    const trigger = loc({ innerText: vi.fn(async () => "Newest") });
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-sort-trigger") return trigger;
      if (query.kind === "role" && query.value === "menu") return loc({ count: vi.fn(async () => 0) });
      return undefined;
    });
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsSortTriggerText()).resolves.toBe("Newest");
    await driver.viewsSortOpen();
    expect(trigger.click).toHaveBeenCalled();
    expect(askedWith(state, "role", "menuitem")).toHaveLength(1);
  });

  it("does not click the trigger when the menu is already open", async () => {
    const trigger = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-sort-trigger") return trigger;
      if (query.kind === "role" && query.value === "menu") return loc({ count: vi.fn(async () => 1) });
      return undefined;
    });
    await new WebNewDriver(state.page).viewsSortOpen();
    expect(trigger.click).not.toHaveBeenCalled();
  });

  it("reads the menu entries", async () => {
    const items = loc({ allInnerTexts: vi.fn(async () => ["Name", " Created "]) });
    const state = harness((query) => (query.kind === "role" && query.value === "menuitem" ? items : undefined));
    await expect(new WebNewDriver(state.page).viewsSortMenuTexts()).resolves.toEqual(["Name", "Created"]);
  });

  it("marks an entry only when it renders a check glyph", async () => {
    const checked = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 1) })) });
    const unchecked = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 0) })) });
    const absent = loc({ count: vi.fn(async () => 0) });
    const withItem = (item: Locator) =>
      harness((query) => (query.kind === "role" && query.value === "menuitem" ? item : undefined)).page;
    await expect(new WebNewDriver(withItem(checked)).viewsSortMenuSelected("Name")).resolves.toBe(true);
    await expect(new WebNewDriver(withItem(unchecked)).viewsSortMenuSelected("Name")).resolves.toBe(false);
    await expect(new WebNewDriver(withItem(absent)).viewsSortMenuSelected("Name")).resolves.toBe(false);
  });

  it("picks an entry and waits for the menu to close", async () => {
    const item = loc();
    const menu = loc();
    const state = harness((query) => {
      if (query.kind === "role" && query.value === "menuitem") return item;
      if (query.kind === "role" && query.value === "menu") return menu;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsSortPick("Name");
    expect(item.click).toHaveBeenCalled();
    expect(menu.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });
});

describe("views filter panel and chips", () => {
  it("opens the panel only when it is closed", async () => {
    const panel = loc({ count: vi.fn(async () => 0) });
    const trigger = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-filter-panel") return panel;
      if (query.kind === "testid" && query.value === "project-views-filter-trigger") return trigger;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsFiltersOpen();
    expect(trigger.click).toHaveBeenCalled();
    expect(panel.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("leaves an open panel alone", async () => {
    const panel = loc();
    const trigger = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-filter-panel") return panel;
      if (query.kind === "testid" && query.value === "project-views-filter-trigger") return trigger;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsFiltersOpen();
    expect(trigger.click).not.toHaveBeenCalled();
  });

  it("reads the panel text", async () => {
    const panel = loc({ innerText: vi.fn(async () => "Created date Created by") });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsFiltersPanelText()).resolves.toBe("Created date Created by");
  });

  it("toggles Favorites and settles the panel closed", async () => {
    const box = loc();
    const panel = loc({ getByRole: vi.fn(() => box) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined
    );
    await new WebNewDriver(state.page).viewsFiltersToggleFavorites();
    expect(box.click).toHaveBeenCalled();
    expect(panel.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("reads and picks date and creator sections", async () => {
    const checkbox = loc({ getAttribute: vi.fn(async () => "Past week") });
    const section = loc({ getByRole: vi.fn(() => checkbox) });
    const panel = loc({ locator: vi.fn(() => section) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined
    );
    const driver = new WebNewDriver(state.page);
    const withLabels = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel"
        ? loc({
            locator: vi.fn(() =>
              loc({
                getByRole: vi.fn(() =>
                  loc({
                    evaluateAll: vi.fn(async (fn: (n: never[]) => unknown) =>
                      fn([
                        { getAttribute: () => "Past week" },
                        { getAttribute: () => " " },
                        { getAttribute: () => null },
                      ] as never[])
                    ),
                  })
                ),
              })
            ),
          })
        : undefined
    );
    await expect(new WebNewDriver(withLabels.page).viewsFiltersDateOptions()).resolves.toEqual(["Past week"]);
    await expect(new WebNewDriver(withLabels.page).viewsFiltersCreatorOptions()).resolves.toEqual(["Past week"]);
    await driver.viewsFiltersPickDate("Past week");
    await driver.viewsFiltersPickCreator("Ada");
    expect(panel.locator).toHaveBeenCalledWith('section[data-section-title="Created date"]');
    expect(panel.locator).toHaveBeenCalledWith('section[data-section-title="Created by"]');
    expect(section.getByRole).toHaveBeenCalledWith("checkbox", { name: "Ada", exact: true });
  });

  it("detects the Access section and searches filters", async () => {
    const present = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 1) })), getByRole: vi.fn(() => loc()) });
    const absent = loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 0) })), getByRole: vi.fn(() => loc()) });
    const panelOf = (panel: Locator) =>
      harness((query) => (query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined));
    await expect(new WebNewDriver(panelOf(present).page).viewsFiltersAccessPresent()).resolves.toBe(true);
    await expect(new WebNewDriver(panelOf(absent).page).viewsFiltersAccessPresent()).resolves.toBe(false);
    const state = panelOf(present);
    await new WebNewDriver(state.page).viewsFiltersSearchType("created");
    expect(present.getByRole).toHaveBeenCalledWith("textbox", { name: "Search filters" });
  });

  it("closes the panel by clicking outside", async () => {
    const panel = loc();
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined
    );
    await new WebNewDriver(state.page).viewsFiltersClose();
    expect(state.mouseClick).toHaveBeenCalledWith(2, 2);
    expect(panel.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("does nothing to close an absent panel", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? ABSENT() : undefined
    );
    await new WebNewDriver(state.page).viewsFiltersClose();
    expect(state.mouseClick).not.toHaveBeenCalled();
  });

  it("reads the chip strip", async () => {
    const group = loc({ getByRole: vi.fn(() => loc()) });
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "project-views-applied-filters") return loc();
      if (query.kind === "selector" && query.value === "[data-chip-dimension]") return group;
      return undefined;
    });
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsChipsVisible()).resolves.toBe(true);
    await driver.viewsChipRemoveValue("Created at", "Past week");
    await driver.viewsChipRemoveDimension("created");
    expect(group.getByRole).toHaveBeenCalledWith("button", { name: "Remove Past week", exact: true });
    expect(asked(state).filter((entry) => entry === "selector:[data-chip-dimension]")).toHaveLength(2);
    const filterOptions = (group.filter as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![0] as {
      has: { called: boolean };
    };
    expect(filterOptions.has).toBeDefined();
  });

  it("clears every chip and waits for the strip to go", async () => {
    const strip = loc();
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-applied-filters" ? strip : undefined
    );
    await new WebNewDriver(state.page).viewsChipsClearAll();
    expect(strip.getByRole).toHaveBeenCalledWith("button", { name: "Clear all", exact: true });
    expect(strip.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("skips clearing an absent strip", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-applied-filters" ? ABSENT() : undefined
    );
    await new WebNewDriver(state.page).viewsChipsClearAll();
    expect(askedWith(state, "role", "button")).toHaveLength(0);
  });
});

describe("views empty, gate and no-match states", () => {
  it("reports the zero-state action and whether it is enabled", async () => {
    const enabled = harness();
    await expect(new WebNewDriver(enabled.page).viewsEmptyCreateVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(enabled.page).viewsEmptyCreateEnabled()).resolves.toBe(true);
    const disabled = harness();
    (disabled.node as never as Stub).isEnabled = vi.fn(async () => false);
    await expect(new WebNewDriver(disabled.page).viewsEmptyCreateEnabled()).resolves.toBe(false);
    const absent = harness((query) =>
      query.kind === "role" && query.value === "button" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsEmptyCreateEnabled()).resolves.toBe(false);
  });

  it("opens the create dialog from the zero state", async () => {
    const state = harness();
    await new WebNewDriver(state.page).viewsEmptyCreateOpen();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(askedWith(state, "role", "heading")[0]!.options).toEqual({ name: "Create View" });
  });

  it("reads the no-match title, or nothing when the card is absent", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["No views match"]);
    await expect(new WebNewDriver(state.page).viewsNoMatchTitle()).resolves.toBe("No views match");
    const empty = harness();
    (empty.node as never as Stub).allInnerTexts = vi.fn(async () => []);
    await expect(new WebNewDriver(empty.page).viewsNoMatchTitle()).resolves.toBe("");
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "project-views-no-match" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsNoMatchTitle()).resolves.toBe("");
  });

  it("reads the gate title and its Manage features action", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).viewsGateManageVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).viewsGateManageEnabled()).resolves.toBe(true);
    const disabled = harness();
    (disabled.node as never as Stub).isEnabled = vi.fn(async () => false);
    await expect(new WebNewDriver(disabled.page).viewsGateManageEnabled()).resolves.toBe(false);
    const absent = harness((query) =>
      query.kind === "role" && query.value === "button" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsGateManageEnabled()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).viewsGateManageVisible()).resolves.toBe(false);
  });

  it("opens Manage features and waits for the next document", async () => {
    const state = harness();
    await new WebNewDriver(state.page).viewsGateManageOpen();
    expect((state.node as never as Stub).click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(state.page.waitForLoadState as ReturnType<typeof vi.fn>).toHaveBeenCalledWith("domcontentloaded");
  });
});

describe("views detail chrome", () => {
  it("reads the saved-view breadcrumb, or nothing without a header", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Acme", "Apollo", "Views", "Sprint"]);
    await expect(new WebNewDriver(state.page).viewsDetailBreadcrumb()).resolves.toEqual([
      "Acme",
      "Apollo",
      "Views",
      "Sprint",
    ]);
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "saved-view-header" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsDetailBreadcrumb()).resolves.toEqual([]);
  });

  it("reads the missing-view card and goes back to the list", async () => {
    const state = harness();
    (state.node as never as Stub).innerText = vi.fn(async () => "This view no longer exists");
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsDetailErrorTitle()).resolves.toBe("This view no longer exists");
    await driver.viewsDetailErrorBack();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(askedWith(state, "role", "button")[0]!.options).toEqual({ name: "Back to views" });
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "saved-view-missing" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsDetailErrorTitle()).resolves.toBe("");
  });

  it("opens the saved-view switcher only when closed and picks an option", async () => {
    const open = harness((query) =>
      query.kind === "role" && query.value === "listbox" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await new WebNewDriver(open.page).viewsDetailSwitcherOpen("Sprint");
    expect((open.node as never as Stub).click).toHaveBeenCalled();
    expect(asked(open)).toContain("testid:saved-view-header");

    const closed = harness();
    await new WebNewDriver(closed.page).viewsDetailSwitcherOpen("Sprint");
    expect((closed.node as never as Stub).click).not.toHaveBeenCalled();

    const picked = harness();
    await new WebNewDriver(picked.page).viewsDetailSwitcherPick("Board");
    const node = picked.node as never as Stub;
    expect(node.click).toHaveBeenCalled();
    expect(node.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("reads switcher options and searches it", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Sprint", "Board"]);
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsDetailSwitcherOptions()).resolves.toEqual(["Sprint", "Board"]);
    await expect(driver.viewsDetailSwitcherSearchVisible()).resolves.toBe(true);
    await driver.viewsDetailSwitcherSearch("bo");
    expect((state.node as never as Stub).fill).toHaveBeenCalledWith("bo");
    expect(askedWith(state, "role", "combobox")).toHaveLength(2);
    expect(askedWith(state, "role", "listbox")[0]!.options).toEqual({ name: "Saved views" });
  });

  it("reports the lock marker", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).viewsDetailLockVisible()).resolves.toBe(true);
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "saved-view-lock" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).viewsDetailLockVisible()).resolves.toBe(false);
  });

  it("finds the active layout from a segmented control", async () => {
    const state = harness((query) =>
      query.kind === "selector"
        ? query.value === '[data-layout="calendar"][data-active="true"]'
          ? loc()
          : loc({ count: vi.fn(async () => 0) })
        : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailLayoutActive()).resolves.toBe(2);
  });

  it("finds the active layout from the dropdown label", async () => {
    const state = harness((query) => {
      if (query.kind === "selector") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "role" && query.value === "button") {
        return loc({
          getAttribute: vi.fn(async (name: string) =>
            name === "aria-label" ? "Change issue layout, current Board" : null
          ),
        });
      }
      return undefined;
    });
    await expect(new WebNewDriver(state.page).viewsDetailLayoutActive()).resolves.toBe(1);
  });

  it("reports no layout when neither affordance marks one", async () => {
    const state = harness((query) => {
      if (query.kind === "selector" || query.value === "button") return loc({ count: vi.fn(async () => 0) });
      return undefined;
    });
    await expect(new WebNewDriver(state.page).viewsDetailLayoutActive()).resolves.toBe(-1);
  });

  it("picks a segmented layout and polls until it is active", async () => {
    const state = harness();
    await new WebNewDriver(state.page).viewsDetailLayoutPick(0);
    const node = state.node as never as Stub;
    expect(node.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(state.asks.some((entry) => entry.includes('[data-layout="list"][data-active="true"]'))).toBe(true);
  });

  it("picks a layout from the dropdown when there is no segment", async () => {
    const state = harness((query) => {
      if (query.kind === "selector") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "role" && query.value === "button") {
        return loc({
          getAttribute: vi.fn(async (name: string) =>
            name === "aria-label" ? "Change issue layout, current Board" : null
          ),
        });
      }
      return undefined;
    });
    await expect(new WebNewDriver(state.page).viewsDetailLayoutPick(1)).resolves.toBeUndefined();
    expect(askedWith(state, "role", "menuitem")[0]!.options).toEqual({ name: "Board", exact: true });
    expect((state.node as never as Stub).click).toHaveBeenCalled();
  });

  it("refuses a layout index outside the list", async () => {
    await expect(new WebNewDriver(harness().page).viewsDetailLayoutPick(9)).rejects.toThrow(/no layout at index 9/);
    await expect(new WebNewDriver(harness().page).viewsDetailLayoutPick(-1)).rejects.toThrow(/no layout at index -1/);
  });

  it("reports layout and display affordances", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "layout-switcher-segmented" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailLayoutVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).viewsDetailDisplayVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).viewsDetailAddVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(state.page).viewsDetailFiltersToggleVisible()).resolves.toBe(true);
    const absent = harness(() => ABSENT());
    await expect(new WebNewDriver(absent.page).viewsDetailLayoutVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).viewsDetailDisplayVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).viewsDetailAddVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).viewsDetailFiltersToggleVisible()).resolves.toBe(false);
    expect(asked(state)).toContain("testid:layout-switcher-segmented");
    expect(asked(state)).toContain("testid:layout-switcher-dropdown");
  });

  it("reads the empty table title", async () => {
    await expect(new WebNewDriver(harness().page).viewsDetailEmptyTitle()).resolves.toBe(
      "View work items will appear here"
    );
    const absent = harness(() => ABSENT());
    await expect(new WebNewDriver(absent.page).viewsDetailEmptyTitle()).resolves.toBe("");
  });

  it("reads the display panel labels and closes it", async () => {
    const panel = loc({
      locator: vi.fn((selector: string) =>
        selector === "button"
          ? loc({ evaluateAll: vi.fn(async () => []) })
          : loc({ evaluateAll: vi.fn(async () => ["Properties", "Grouping"]) })
      ),
      evaluateAll: vi.fn(async () => []),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual(["Properties", "Grouping"]);
    expect(state.press).toHaveBeenCalledWith("Escape");
    // The panel stub itself is what the panel read touches; the trigger is
    // on the page node, so assert it was queried and clicked there.
    expect(asked(state)).toContain("testid:display-options-panel");
  });

  it("returns no labels when the display trigger is absent", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-trigger"
        ? loc({ count: vi.fn(async () => 0) })
        : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual([]);
  });

  it("adds a filter through the issues header", async () => {
    const option = loc();
    const properties = loc({ getByRole: vi.fn(() => option) });
    const state = harness((query) => {
      if (query.kind === "role" && query.value === "listbox") return properties;
      return undefined;
    });
    const driver = new WebNewDriver(state.page);
    await driver.viewsDetailFilterAdd("Priority", "High");
    expect(option.click).toHaveBeenCalled();
    expect(state.queries.some((query) => query.kind === "selector" && query.value === "xpath=..")).toBe(false);
  });

  it("refreshes the view and saves it as a new one", async () => {
    const state = harness();
    const driver = new WebNewDriver(state.page);
    await driver.viewsDetailUpdateView();
    expect((state.node as never as Stub).waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    await expect(driver.viewsDetailSaveAsVisible()).resolves.toBe(true);
    await driver.viewsDetailSaveAsClick();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
  });

  it("opens the new work item form and reports the listed issue", async () => {
    const state = harness();
    const driver = new WebNewDriver(state.page);
    await driver.viewsDetailAddClick();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    await expect(driver.viewsDetailShowsIssue("Alpha issue")).resolves.toBe(true);
    const absent = harness((query) => (query.kind === "text" ? loc({ count: vi.fn(async () => 0) }) : undefined));
    await expect(new WebNewDriver(absent.page).viewsDetailShowsIssue("Gone")).resolves.toBe(false);
  });
});

describe("workspace views list", () => {
  it("reads custom row names in order, skipping blanks", async () => {
    const rows = loc({
      count: vi.fn(async () => 2),
      nth: vi.fn((index: number) => loc({ innerText: vi.fn(async () => (index === 0 ? " Sprint " : "")) })),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "workspace-views-custom-row" ? rows : undefined
    );
    await expect(new WebNewDriver(state.page).wsViewsListNames()).resolves.toEqual(["Sprint"]);
    expect(rows.nth).toHaveBeenCalledTimes(2);
  });

  it("reads the built-in row labels", async () => {
    const state = harness();
    (state.node as never as Stub).evaluateAll = vi.fn(async (fn: (nodes: never[]) => unknown) =>
      fn([{ textContent: " All work items " }, { textContent: "" }, { textContent: " My views" }] as never[])
    );
    await expect(new WebNewDriver(state.page).wsViewsDefaultNames()).resolves.toEqual(["All work items", "My views"]);
  });

  it("reads a custom row description and returns an empty string when the row or subtitle is absent", async () => {
    const withSubtitle = harness((query) =>
      query.kind === "testid" && query.value === "workspace-views-custom-row"
        ? loc({
            count: vi.fn(async () => 1),
            locator: vi.fn(() =>
              loc({
                count: vi.fn(async () => 2),
                nth: vi.fn(() => loc({ innerText: vi.fn(async () => "Owned by Ada") })),
              })
            ),
          })
        : undefined
    );
    await expect(new WebNewDriver(withSubtitle.page).wsViewsRowDescription("Sprint")).resolves.toBe("Owned by Ada");
    const noRow = harness((query) =>
      query.kind === "testid" && query.value === "workspace-views-custom-row" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(noRow.page).wsViewsRowDescription("Sprint")).resolves.toBe("");
    const noSubtitle = harness((query) =>
      query.kind === "testid" && query.value === "workspace-views-custom-row"
        ? loc({ locator: vi.fn(() => loc({ count: vi.fn(async () => 1) })) })
        : undefined
    );
    await expect(new WebNewDriver(noSubtitle.page).wsViewsRowDescription("Sprint")).resolves.toBe("");
  });

  it("opens a custom row's menu and picks an entry", async () => {
    const state = harness();
    const driver = new WebNewDriver(state.page);
    await driver.wsViewsRowMenuOpen("Sprint");
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(asked(state)).toContain("testid:workspace-views-row-menu-trigger");
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Rename", "Delete"]);
    await expect(driver.wsViewsRowMenuEntries()).resolves.toEqual(["Rename", "Delete"]);
    await driver.wsViewsRowMenuPick("Delete");
    expect(askedWith(state, "role", "menuitem").some((q) => q.options?.["name"] === "Delete")).toBe(true);
  });

  it("fills the workspace search box", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsSearchFill("sprint");
    expect((state.node as never as Stub).fill).toHaveBeenCalledWith("sprint");
    expect(asked(state)).toContain("testid:workspace-views-search");
    expect(askedWith(state, "role", "textbox")[0]!.options).toEqual({ name: "Search views" });
  });

  it("reports the tab strip, star and default-row menu", async () => {
    const state = harness();
    const driver = new WebNewDriver(state.page);
    await expect(driver.wsViewsStripVisible()).resolves.toBe(true);
    await expect(driver.wsViewsListStarVisible()).resolves.toBe(true);
    await expect(driver.wsViewsDefaultRowHasMenu("All work items")).resolves.toBe(true);
    const absent = harness((query) =>
      query.kind === "testid" &&
      ["workspace-views-tab-strip", "project-views-row-star", "workspace-views-default-row"].includes(query.value)
        ? ABSENT()
        : undefined
    );
    await expect(new WebNewDriver(absent.page).wsViewsStripVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).wsViewsListStarVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent.page).wsViewsDefaultRowHasMenu("All work items")).resolves.toBe(false);
  });

  it("opens a custom row and settles on the detail header", async () => {
    const header = loc();
    const state = harness((query) =>
      query.kind === "selector" &&
      query.value === '[data-testid="workspace-view-header"], [data-testid="saved-view-missing"]'
        ? header
        : undefined
    );
    await new WebNewDriver(state.page).wsViewsRowOpen("Sprint");
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(header.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 120_000 });
  });

  it("reads the dialog's display options", async () => {
    const panel = loc({
      locator: vi.fn(() => loc({ evaluateAll: vi.fn(async () => []) })),
      evaluateAll: vi.fn(async () => []),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).wsViewsDialogDisplayOptions()).resolves.toEqual([]);
    // The panel is already up, so the trigger is not clicked.
    expect((state.node as never as Stub).click).not.toHaveBeenCalled();
    expect(state.press).toHaveBeenCalledWith("Escape");
  });

  it("waits for the skeleton observer and reports whether it saw one", async () => {
    const seen = harness();
    await expect(new WebNewDriver(seen.page).wsViewsSkeletonFlashed(5)).resolves.toBe(true);
    const missed = harness();
    (missed.page.waitForFunction as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => {
      throw new Error("timeout");
    });
    missed.evaluate.mockImplementationOnce(async () => false);
    await expect(new WebNewDriver(missed.page).wsViewsSkeletonFlashed(5)).resolves.toBe(false);
    const late = harness();
    (late.page.waitForFunction as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => {
      throw new Error("timeout");
    });
    late.evaluate.mockImplementationOnce(async (fn: () => boolean) => {
      vi.stubGlobal("window", { __parityViewsSkeletonSeen: true });
      try {
        return fn();
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(late.page).wsViewsSkeletonFlashed(5)).resolves.toBe(true);
  });

  it("arms the skeleton observer and holds the next workspace views read", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsDelayNextList(5);
    expect(state.addInitScript).toHaveBeenCalledWith(expect.any(Function));
    const [match, handler] = state.route.mock.calls[0] as [(url: URL) => boolean, (route: Route) => Promise<void>];
    expect(match(new URL("https://api.test/api/workspaces/w/views/"))).toBe(true);
    expect(match(new URL("https://api.test/api/workspaces/w/views/x/"))).toBe(false);
    const get = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(get as unknown as Route);
    expect(get.continue).toHaveBeenCalled();
  });

  it("fails the next write once and lets reads through", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsFailNextWrite(500);
    expect(state.route).toHaveBeenCalledWith(expect.any(Function), expect.any(Function));
    const [, handler] = state.route.mock.calls[0] as [(url: URL) => boolean, (route: Route) => Promise<void>];
    const read = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined), fulfill: vi.fn() };
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalled();
    const write = {
      request: () => ({ method: () => "POST" }),
      continue: vi.fn(async () => undefined),
      fulfill: vi.fn(async () => undefined),
    };
    await handler(write as unknown as Route);
    expect(write.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 500 }));
    expect(state.unroute).toHaveBeenCalled();
  });
});

describe("workspace views detail", () => {
  it("reads the header crumbs, or nothing without a header", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Acme", "Workspace views", "Sprint"]);
    await expect(new WebNewDriver(state.page).wsViewsDetailCrumbs()).resolves.toEqual([
      "Acme",
      "Workspace views",
      "Sprint",
    ]);
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "workspace-view-header" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).wsViewsDetailCrumbs()).resolves.toEqual([]);
  });

  it("opens the workspace switcher only when closed and picks an option", async () => {
    const closed = harness((query) =>
      query.kind === "role" && query.value === "listbox" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await new WebNewDriver(closed.page).wsViewsDetailSwitcherOpen("Sprint");
    expect((closed.node as never as Stub).click).toHaveBeenCalled();
    expect(asked(closed)).toContain("testid:workspace-view-header");
    const open = harness();
    await new WebNewDriver(open.page).wsViewsDetailSwitcherOpen("Sprint");
    expect((open.node as never as Stub).click).not.toHaveBeenCalled();
    const picked = harness();
    await new WebNewDriver(picked.page).wsViewsDetailSwitcherPick("Board");
    expect((picked.node as never as Stub).click).toHaveBeenCalled();
    expect(askedWith(picked, "role", "listbox")[0]!.options).toEqual({ name: "Workspace views" });
  });

  it("reads switcher options and searches it", async () => {
    const state = harness();
    (state.node as never as Stub).allInnerTexts = vi.fn(async () => ["Sprint", "Board"]);
    const driver = new WebNewDriver(state.page);
    await expect(driver.wsViewsDetailSwitcherOptions()).resolves.toEqual(["Sprint", "Board"]);
    await expect(driver.wsViewsDetailSwitcherSearchVisible()).resolves.toBe(true);
    await driver.wsViewsDetailSwitcherSearch("bo");
    expect((state.node as never as Stub).fill).toHaveBeenCalledWith("bo");
  });

  it("reports header affordances and an absent header as false", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "layout-switcher-segmented" ? ABSENT() : undefined
    );
    const driver = new WebNewDriver(state.page);
    await expect(driver.wsViewsDetailLayoutVisible()).resolves.toBe(true);
    await expect(driver.wsViewsDetailDisplayVisible()).resolves.toBe(true);
    await expect(driver.wsViewsDetailAddVisible()).resolves.toBe(true);
    await expect(driver.wsViewsDetailFiltersToggleVisible()).resolves.toBe(true);
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "workspace-view-header" ? ABSENT() : undefined
    );
    const nothing = new WebNewDriver(absent.page);
    await expect(nothing.wsViewsDetailLayoutVisible()).resolves.toBe(false);
    await expect(nothing.wsViewsDetailDisplayVisible()).resolves.toBe(false);
    await expect(nothing.wsViewsDetailAddVisible()).resolves.toBe(false);
    await expect(nothing.wsViewsDetailFiltersToggleVisible()).resolves.toBe(false);
  });

  it("falls back to the page add-filter control when the header has no toggle", async () => {
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "rich-filter-toggle") return loc({ count: vi.fn(async () => 0) });
      return undefined;
    });
    await expect(new WebNewDriver(state.page).wsViewsDetailFiltersToggleVisible()).resolves.toBe(true);
    expect(asked(state)).toContain("testid:rich-filter-add");
  });

  it("opens the header's add action", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsDetailAddClick();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(asked(state)).toContain("testid:workspace-view-add");
  });

  it("opens the detail menu once and reads its entries", async () => {
    const state = harness((query) =>
      query.kind === "role" && query.value === "menuitem" ? loc({ count: vi.fn(async () => 0) }) : undefined
    );
    await new WebNewDriver(state.page).wsViewsDetailMenuOpen();
    expect((state.node as never as Stub).click).toHaveBeenCalled();
    expect(asked(state)).toContain("testid:workspace-view-menu-trigger");
    const open = harness();
    (open.node as never as Stub).allInnerTexts = vi.fn(async () => ["Copy link", "Open in new tab"]);
    await expect(new WebNewDriver(open.page).wsViewsDetailMenuEntries()).resolves.toEqual([
      "Copy link",
      "Open in new tab",
    ]);
  });

  it("copies the view link through the clipboard", async () => {
    const state = harness();
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) =>
      String(fn).includes("readText") ? "https://app.test/acme/workspace-views/1" : undefined
    );
    await expect(new WebNewDriver(state.page).wsViewsDetailCopyLink()).resolves.toBe(
      "https://app.test/acme/workspace-views/1"
    );
    expect(state.grantPermissions).toHaveBeenCalledWith(["clipboard-read", "clipboard-write"]);
    expect(askedWith(state, "role", "menuitem").some((q) => q.options?.["name"] === "Copy link")).toBe(true);
  });

  it("returns the popup URL and closes the tab", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).wsViewsDetailOpenNewTabHref()).resolves.toBe(
      "https://app.test/acme/workspace-views/1"
    );
    expect(askedWith(state, "role", "menuitem").some((q) => q.options?.["name"] === "Open in new tab")).toBe(true);
  });

  it("reads the missing view and goes back to all work items", async () => {
    const state = harness();
    (state.node as never as Stub).innerText = vi.fn(async () => "View not found");
    const driver = new WebNewDriver(state.page);
    await expect(driver.wsViewsDetailErrorTitle()).resolves.toBe("View not found");
    await driver.wsViewsDetailErrorBack();
    expect(askedWith(state, "role", "button")[0]!.options).toEqual({ name: "Go to All work items", exact: true });
    const absent = harness((query) =>
      query.kind === "testid" && query.value === "saved-view-missing" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(absent.page).wsViewsDetailErrorTitle()).resolves.toBe("");
  });
});

describe("views settings and the flag coupling", () => {
  it("waits for the settings panel before reading the toggle", async () => {
    const state = harness();
    (state.node as never as Stub).getAttribute = vi.fn(async () => "true");
    await expect(new WebNewDriver(state.page).viewsSettingsViewsToggleValue()).resolves.toBe(true);
    expect((state.node as never as Stub).waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
    expect(asked(state)).toContain("testid:project-feature-settings");
    expect(askedWith(state, "role", "switch")[0]!.options).toEqual({ name: "Enable views" });
    const off = harness();
    (off.node as never as Stub).getAttribute = vi.fn(async () => "false");
    await expect(new WebNewDriver(off.page).viewsSettingsViewsToggleValue()).resolves.toBe(false);
  });

  it("flips the toggle", async () => {
    const state = harness();
    await new WebNewDriver(state.page).viewsSettingsViewsToggleFlip();
    expect((state.node as never as Stub).click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("decides the settings gate from the denied card", async () => {
    const state = harness();
    await expect(new WebNewDriver(state.page).viewsSettingsNotAuthorized()).resolves.toBe(true);
    const allowed = harness((query) =>
      query.kind === "testid" && query.value === "project-settings-denied" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(allowed.page).viewsSettingsNotAuthorized()).resolves.toBe(false);
  });
});

describe("views negative-path helpers", () => {
  it("drags between two rows, and no-ops when a row is missing", async () => {
    const source = loc();
    const target = loc();
    let call = 0;
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-row"
        ? loc({ filter: vi.fn(() => (call++ === 0 ? source : target)) })
        : undefined
    );
    await new WebNewDriver(state.page).viewsListDragRow("Alpha", "Beta");
    expect(source.dragTo).toHaveBeenCalledWith(target, { timeout: 30_000 });
    const missing = harness((query) =>
      query.kind === "testid" && query.value === "project-views-row" ? ABSENT() : undefined
    );
    await expect(new WebNewDriver(missing.page).viewsListDragRow("Alpha", "Beta")).resolves.toBeUndefined();
  });

  it("walks the dialog tab order and stops when focus wraps", async () => {
    const names = ["Title", "Description", "Create"];
    let index = 0;
    const state = harness();
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) => {
      if (!String(fn).includes("activeElement")) return undefined;
      const name = names[index % names.length]!;
      index += 1;
      return name;
    });
    await expect(new WebNewDriver(state.page).viewsDialogTabOrder()).resolves.toEqual([
      "Title",
      "Description",
      "Create",
    ]);
    expect(state.press).toHaveBeenCalledWith("Tab");
  });

  it("skips unnamed focus stops and stops at the 24-step ceiling", async () => {
    let step = 0;
    const state = harness();
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) => {
      if (!String(fn).includes("activeElement")) return undefined;
      step += 1;
      return `step-${step}`;
    });
    const order = await new WebNewDriver(state.page).viewsDialogTabOrder();
    expect(order).toHaveLength(24);
    expect(order[0]).toBe("step-1");
    expect(order[23]).toBe("step-24");
  });

  it("reports export/import affordances from buttons or links", async () => {
    const byButton = harness();
    await expect(new WebNewDriver(byButton.page).viewsPageExportImportVisible()).resolves.toBe(true);
    const byLink = harness((query) => (query.kind === "role" && query.value === "button" ? ABSENT() : undefined));
    await expect(new WebNewDriver(byLink.page).viewsPageExportImportVisible()).resolves.toBe(true);
    const neither = harness(() => ABSENT());
    await expect(new WebNewDriver(neither.page).viewsPageExportImportVisible()).resolves.toBe(false);
  });
});

/** A locator whose waitFor rejects, as a timeout would. */
function rejecting(overrides: Stub = {}): Locator {
  return loc({ waitFor: vi.fn(async () => Promise.reject(new Error("timeout"))), ...overrides });
}

describe("views defensive branches", () => {
  it("treats a rejected display-panel teardown as settled", async () => {
    const panel = loc({
      locator: vi.fn(() => loc({ evaluateAll: vi.fn(async () => []) })),
      evaluateAll: vi.fn(async () => []),
      waitFor: vi.fn(async (options: { state: string }) =>
        options.state === "visible" ? undefined : Promise.reject(new Error("timeout"))
      ),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual([]);
    expect(panel.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("sorts section titles and skips expand toggles and duplicates", async () => {
    const titles = loc({ evaluateAll: vi.fn(async () => ["Properties", "", "Grouping"]) });
    const buttons = loc({
      evaluateAll: vi.fn(async () => [
        { label: "Collapse Properties", expand: true },
        { label: "", expand: false },
        { label: "Properties", expand: false },
        { label: "Show sub-items", expand: false },
      ]),
    });
    const panel = loc({
      locator: vi.fn((selector: string) => (selector === "button" ? buttons : titles)),
      waitFor: vi.fn(async () => undefined),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual([
      "Properties",
      "Grouping",
      "Show sub-items",
    ]);
  });

  it("runs the search-focus evaluate callback against the active element", async () => {
    const state = harness();
    (state.node as never as Stub).evaluate = vi.fn(async (fn: (node: unknown) => unknown) => {
      const active = { focus: () => undefined };
      vi.stubGlobal("document", { activeElement: active });
      try {
        return fn(active);
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(state.page).viewsSearchFocused()).resolves.toBe(true);
    (state.node as never as Stub).evaluate = vi.fn(async (fn: (node: unknown) => unknown) => {
      vi.stubGlobal("document", { activeElement: { focus: () => undefined } });
      try {
        return fn({ focus: () => undefined });
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(state.page).viewsSearchFocused()).resolves.toBe(false);
  });

  it("settles every close wait even when the menu or listbox never disappears", async () => {
    const state = harness();
    (state.node as never as Stub).waitFor = vi.fn(async () => Promise.reject(new Error("timeout")));
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsSortPick("Name")).resolves.toBeUndefined();
    await expect(driver.viewsLayoutPick("List")).resolves.toBeUndefined();
    await expect(driver.viewsDetailSwitcherPick("Sprint")).resolves.toBeUndefined();
    await expect(driver.wsViewsDetailSwitcherPick("Sprint")).resolves.toBeUndefined();
    await expect(driver.wsViewsRowMenuPick("Rename")).resolves.toBeUndefined();
  });

  it("settles the chip-strip and update-view teardowns on rejection", async () => {
    const strip = rejecting();
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-applied-filters" ? strip : undefined
    );
    await expect(new WebNewDriver(state.page).viewsChipsClearAll()).resolves.toBeUndefined();
    const updateState = harness();
    (updateState.node as never as Stub).waitFor = vi.fn(async (options: { state: string }) =>
      options.state === "visible" ? undefined : Promise.reject(new Error("x"))
    );
    await expect(new WebNewDriver(updateState.page).viewsDetailUpdateView()).resolves.toBeUndefined();
  });

  it("opens the display panel when it starts closed", async () => {
    const closedFirst = loc({
      count: vi.fn(async () => 1),
      isVisible: vi.fn(async () => false),
      locator: vi.fn(() => loc({ evaluateAll: vi.fn(async () => []) })),
      evaluateAll: vi.fn(async () => []),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? closedFirst : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual([]);
    expect((state.node as never as Stub).click).toHaveBeenCalled();
  });

  it("opens the workspace dialog display panel when it starts closed", async () => {
    const closedFirst = loc({
      count: vi.fn(async () => 1),
      isVisible: vi.fn(async () => false),
      locator: vi.fn(() => loc({ evaluateAll: vi.fn(async () => []) })),
      evaluateAll: vi.fn(async () => []),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? closedFirst : undefined
    );
    await expect(new WebNewDriver(state.page).wsViewsDialogDisplayOptions()).resolves.toEqual([]);
    expect((state.node as never as Stub).click).toHaveBeenCalled();
  });

  it("opens the issues filter row and picks a listed value", async () => {
    const toggle = loc({ getAttribute: vi.fn(async () => "false") });
    const direct = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "rich-filter-toggle") return toggle;
      if (query.kind === "role" && query.value === "checkbox") return direct;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsDetailFilterAdd("Priority", "High");
    expect(toggle.click).toHaveBeenCalled();
    expect(direct.click).toHaveBeenCalled();
    expect(askedWith(state, "role", "button").some((q) => q.options?.["name"] === "--")).toBe(false);
  });

  it("opens the value set before picking when the value is not listed yet", async () => {
    let seen = 0;
    const valueSet = loc();
    const state = harness((query) => {
      if (query.kind === "testid" && query.value === "rich-filter-toggle") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "testid" && query.value === "rich-filter-add") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "role" && query.value === "checkbox") {
        seen += 1;
        return loc({ count: vi.fn(async () => (seen === 1 ? 0 : 1)) });
      }
      if (query.kind === "role" && query.value === "button") return valueSet;
      return undefined;
    });
    await new WebNewDriver(state.page).viewsDetailFilterAdd("Priority", "High");
    expect(valueSet.click).toHaveBeenCalled();
    expect(askedWith(state, "role", "button").some((q) => q.options?.["name"] === "Add filter")).toBe(true);
  });

  it("reports a layout with an unrecognised dropdown label as none", async () => {
    const state = harness((query) => {
      if (query.kind === "selector") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "role" && query.value === "button") {
        return loc({
          getAttribute: vi.fn(async (name: string) => (name === "aria-label" ? "Change issue layout" : null)),
        });
      }
      return undefined;
    });
    await expect(new WebNewDriver(state.page).viewsDetailLayoutActive()).resolves.toBe(-1);
  });

  it("drives the skeleton observer's in-page callbacks", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsDelayNextList(1);
    const init = state.addInitScript.mock.calls[0]![0] as () => void;

    type Listener = () => void;
    const makeWindow = (readyState: string) => {
      const listeners = new Map<string, Listener>();
      const observers: Array<() => void> = [];
      const win = {
        document: {
          readyState,
          documentElement: {},
          querySelector: (selector: string) => (selector.includes("project-views-skeleton") ? {} : null),
          addEventListener: (type: string, handler: Listener) => listeners.set(type, handler),
        },
        MutationObserver: class {
          constructor(private readonly scan: () => void) {
            observers.push(scan);
          }
          observe(): void {}
        },
      };
      return { win, listeners, observers };
    };

    const loading = makeWindow("loading");
    vi.stubGlobal("window", loading.win);
    vi.stubGlobal("document", loading.win.document);
    vi.stubGlobal("MutationObserver", loading.win.MutationObserver);
    init();
    expect((loading.win as unknown as Record<string, unknown>)["__parityViewsSkeletonSeen"]).toBe(false);
    loading.listeners.get("DOMContentLoaded")!();
    expect((loading.win as unknown as Record<string, unknown>)["__parityViewsSkeletonSeen"]).toBe(true);
    expect(loading.observers).toHaveLength(1);

    const ready = makeWindow("complete");
    vi.stubGlobal("window", ready.win);
    vi.stubGlobal("document", ready.win.document);
    vi.stubGlobal("MutationObserver", ready.win.MutationObserver);
    init();
    expect(ready.observers).toHaveLength(1);
    vi.unstubAllGlobals();

    const [match, handler] = state.route.mock.calls[0] as [(url: URL) => boolean, (route: Route) => Promise<void>];
    const post = { request: () => ({ method: () => "POST" }), continue: vi.fn(async () => undefined) };
    await handler(post as unknown as Route);
    expect(post.continue).toHaveBeenCalled();
    expect(match(new URL("https://api.test/api/workspaces/w/views/"))).toBe(true);
    expect(match(new URL("https://api.test/api/workspaces/w/views/1/"))).toBe(false);
    expect(match(new URL("https://api.test/api/projects/p/views/"))).toBe(false);
  });

  it("matches the fail-next-write route on workspaces and views only", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsFailNextWrite(503);
    const [match] = state.route.mock.calls[0] as [(url: URL) => boolean];
    expect(match(new URL("https://api.test/api/workspaces/w/views/"))).toBe(true);
    expect(match(new URL("https://api.test/api/projects/p/views/"))).toBe(false);
  });

  it("runs the in-page skeleton-seen probes", async () => {
    const state = harness();
    (state.page.waitForFunction as ReturnType<typeof vi.fn>).mockImplementationOnce(async (fn: () => boolean) => {
      vi.stubGlobal("window", { __parityViewsSkeletonSeen: true });
      try {
        expect(fn()).toBe(true);
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(state.page).wsViewsSkeletonFlashed(50)).resolves.toBe(true);
    const polling = state.waitForFunction.mock.calls[0]![0] as () => boolean;
    vi.stubGlobal("window", { __parityViewsSkeletonSeen: false });
    try {
      expect(polling()).toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("runs the clipboard read through evaluate", async () => {
    const state = harness();
    const copy = "https://app.test/acme/workspace-views/7";
    state.grantPermissions.mockClear();
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) => {
      vi.stubGlobal("navigator", { clipboard: { readText: async () => copy } });
      try {
        return await fn();
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(state.page).wsViewsDetailCopyLink()).resolves.toBe(copy);
  });

  it("swallows a rejected skeleton poll, panel close, chip teardown and drag", async () => {
    const state = harness();
    const node = state.node as never as Stub;
    node.waitFor = vi.fn(async (options: { state: string }) =>
      options.state === "hidden" ? Promise.reject(new Error("x")) : undefined
    );
    node.dragTo = vi.fn(async () => Promise.reject(new Error("x")));
    node.or = vi.fn(() => node);
    const driver = new WebNewDriver(state.page);
    await expect(driver.viewsFiltersToggleFavorites()).resolves.toBeUndefined();
    await expect(driver.viewsFiltersClose()).resolves.toBeUndefined();
    await expect(driver.viewsSettingsNotAuthorized()).resolves.toBe(true);
    await expect(driver.viewsListDragRow("Alpha", "Beta")).resolves.toBeUndefined();
    await expect(driver.viewsDetailFilterAdd("Priority", "High")).resolves.toBeUndefined();
  });

  it("treats a filter panel with no Favorites pick as a plain reopen", async () => {
    const panel = loc({ waitFor: vi.fn(async () => Promise.reject(new Error("x"))) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-filter-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsFiltersToggleFavorites()).resolves.toBeUndefined();
    expect(panel.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: 30_000 });
  });

  it("reports the gate card title from its heading span", async () => {
    const gate = loc({ locator: vi.fn(() => loc({ innerText: vi.fn(async () => "Views are off") })) });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-gate" ? gate : undefined
    );
    await expect(new WebNewDriver(state.page).viewsGateTitle()).resolves.toBe("Views are off");
  });

  it("reads section titles, skipping ones without a value", async () => {
    const titles = loc({
      evaluateAll: vi.fn(async (fn: (nodes: unknown[]) => unknown) =>
        fn([{ getAttribute: () => null }, { getAttribute: () => "" }, { getAttribute: () => "Grouping" }])
      ),
    });
    const buttons = loc({ evaluateAll: vi.fn(async () => []) });
    const panel = loc({
      locator: vi.fn((selector: string) => (selector === "button" ? buttons : titles)),
      waitFor: vi.fn(async () => undefined),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual(["Grouping"]);
  });

  it("labels a button by its text when it has no aria-label", async () => {
    const titles = loc({ evaluateAll: vi.fn(async () => []) });
    const buttons = loc({
      evaluateAll: vi.fn(async () => [
        {
          label: String(({ textContent: "  Show   sub-items  " } as { textContent: string }).textContent)
            .replace(/\s+/g, " ")
            .trim(),
          expand: false,
        },
      ]),
    });
    const panel = loc({
      locator: vi.fn((selector: string) => (selector === "button" ? buttons : titles)),
      waitFor: vi.fn(async () => undefined),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual(["Show sub-items"]);
  });

  it("counts an absent text as the empty string", async () => {
    const state = harness((query) =>
      query.kind === "testid" && query.value === "project-views-row-access"
        ? loc({ count: vi.fn(async () => 0) })
        : undefined
    );
    await expect(new WebNewDriver(state.page).viewsRowAccess("Alpha")).resolves.toBe("");
  });

  it("names focus stops by aria-label, placeholder, text, then tag, and wraps", async () => {
    const stops = [
      { active: null },
      { active: "body" },
      { ariaLabel: "Icon", placeholder: null, text: "x", tag: "BUTTON" },
      { ariaLabel: "", placeholder: "Title", text: "x", tag: "INPUT" },
      { ariaLabel: null, placeholder: null, text: "  Create   view ", tag: "BUTTON" },
      { ariaLabel: null, placeholder: null, text: "", tag: "DIV" },
      { ariaLabel: "Icon", placeholder: null, text: "x", tag: "BUTTON" },
    ];
    let index = 0;
    const state = harness();
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) => {
      if (!String(fn).includes("activeElement")) return undefined;
      const stop = stops[Math.min(index, stops.length - 1)]!;
      index += 1;
      const body = { tagName: "BODY" };
      let active: unknown = null;
      if (stop.active === "body") active = body;
      else if (stop.active !== null) {
        active = {
          tagName: stop.tag,
          textContent: stop.text,
          getAttribute: (name: string) => (name === "aria-label" ? stop.ariaLabel : stop.placeholder),
        };
      }
      vi.stubGlobal("document", { activeElement: active, body });
      try {
        return fn();
      } finally {
        vi.unstubAllGlobals();
      }
    });
    const order = await new WebNewDriver(state.page).viewsDialogTabOrder();
    expect(order).toEqual(["Icon", "Title", "Create view", "div"]);
  });

  it("falls back to an empty label when a node reports nothing", async () => {
    // A row whose innerText resolves to null reads as "".
    const blank = harness();
    (blank.node as never as Stub).innerText = vi.fn(async () => null);
    await expect(new WebNewDriver(blank.page).viewsRowAccess("Alpha")).resolves.toBe("");

    // A dropdown with no aria-label reports no active layout.
    const noLabel = harness((query) => {
      if (query.kind === "selector") return loc({ count: vi.fn(async () => 0) });
      if (query.kind === "role" && query.value === "button") return loc({ getAttribute: vi.fn(async () => null) });
      return undefined;
    });
    await expect(new WebNewDriver(noLabel.page).viewsDetailLayoutActive()).resolves.toBe(-1);

    // A display button with no aria-label and no text is skipped by name "".
    const titles = loc({ evaluateAll: vi.fn(async () => []) });
    const buttons = loc({
      evaluateAll: vi.fn(async (fn: (nodes: unknown[]) => unknown) =>
        fn([
          { getAttribute: () => null, textContent: null },
          { getAttribute: () => null, textContent: "Show sub-items" },
        ])
      ),
    });
    const panel = loc({
      locator: vi.fn((selector: string) => (selector === "button" ? buttons : titles)),
      waitFor: vi.fn(async () => undefined),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(state.page).viewsDetailDisplayOptions()).resolves.toEqual(["Show sub-items"]);
  });

  it("keeps going when teardown of an already-gone surface rejects", async () => {
    // unroute after the one-shot write fails
    const write = harness();
    write.unroute.mockImplementationOnce(async () => Promise.reject(new Error("x")));
    await new WebNewDriver(write.page).wsViewsFailNextWrite(500);
    const [, writeHandler] = write.route.mock.calls[0] as [(url: URL) => boolean, (route: Route) => Promise<void>];
    await writeHandler({
      request: () => ({ method: () => "DELETE" }),
      fulfill: vi.fn(async () => undefined),
    } as unknown as Route);
    expect(write.unroute).toHaveBeenCalled();

    // the workspace dialog's panel never hides
    const panel = loc({
      locator: vi.fn(() => loc({ evaluateAll: vi.fn(async () => []) })),
      evaluateAll: vi.fn(async () => []),
      waitFor: vi.fn(async (options: { state: string }) =>
        options.state === "visible" ? undefined : Promise.reject(new Error("x"))
      ),
    });
    const dialogState = harness((query) =>
      query.kind === "testid" && query.value === "display-options-panel" ? panel : undefined
    );
    await expect(new WebNewDriver(dialogState.page).wsViewsDialogDisplayOptions()).resolves.toEqual([]);

    // the popup closes itself before we can
    const popupState = harness();
    popupState.waitForEvent.mockImplementationOnce(async () => ({
      url: () => "https://app.test/acme/workspace-views/2",
      close: async () => {
        throw new Error("already closed");
      },
    }));
    await expect(new WebNewDriver(popupState.page).wsViewsDetailOpenNewTabHref()).resolves.toBe(
      "https://app.test/acme/workspace-views/2"
    );

    // the settings gate never paints either card
    const settings = harness();
    (settings.node as never as Stub).waitFor = vi.fn(async () => Promise.reject(new Error("x")));
    await expect(new WebNewDriver(settings.page).viewsSettingsNotAuthorized()).resolves.toBe(true);
  });

  it("runs the default-row label mapping over rows without text", async () => {
    const rows = loc({
      evaluateAll: vi.fn(async (fn: (nodes: unknown[]) => unknown) =>
        fn([{ textContent: null }, { textContent: "  All   work items " }])
      ),
    });
    const state = harness((query) =>
      query.kind === "testid" && query.value === "workspace-views-default-row"
        ? loc({ locator: vi.fn(() => rows) })
        : undefined
    );
    await expect(new WebNewDriver(state.page).wsViewsDefaultNames()).resolves.toEqual(["All work items"]);
  });

  it("boots the skeleton observer straight away once the DOM is ready", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsDelayNextList(1);
    const init = state.addInitScript.mock.calls[0]![0] as () => void;
    type Listener = () => void;
    const listeners = new Map<string, Listener>();
    const observers: Array<{ scan: () => void; options: unknown }> = [];
    const documentElement = { tag: "html" };
    const win = {
      document: {
        readyState: "interactive",
        documentElement,
        querySelector: () => null,
        addEventListener: (type: string, handler: Listener) => listeners.set(type, handler),
      },
      MutationObserver: class {
        constructor(private readonly scan: () => void) {}
        observe(_target: unknown, options: unknown): void {
          observers.push({ scan: this.scan, options });
        }
      },
    };
    vi.stubGlobal("window", win);
    vi.stubGlobal("document", win.document);
    vi.stubGlobal("MutationObserver", win.MutationObserver);
    try {
      init();
    } finally {
      vi.unstubAllGlobals();
    }
    expect(listeners.size).toBe(0);
    expect(observers).toHaveLength(1);
    expect(observers[0]!.options).toEqual({ childList: true, subtree: true });
  });

  it("treats a focus stop with no text content as its tag name", async () => {
    const state = harness();
    let first = true;
    state.evaluate.mockImplementation(async (fn: (arg?: unknown) => unknown) => {
      if (!String(fn).includes("activeElement")) return undefined;
      if (!first) return "button";
      first = false;
      const active = { tagName: "BUTTON", textContent: null, getAttribute: () => null };
      vi.stubGlobal("document", { activeElement: active, body: { tagName: "BODY" } });
      try {
        return fn();
      } finally {
        vi.unstubAllGlobals();
      }
    });
    await expect(new WebNewDriver(state.page).viewsDialogTabOrder()).resolves.toEqual(["button"]);
  });

  it("waits for the skeleton observer with the requested window", async () => {
    const state = harness();
    await new WebNewDriver(state.page).wsViewsSkeletonFlashed(250);
    expect(state.waitForFunction).toHaveBeenCalledWith(expect.any(Function), { timeout: 250, polling: 100 });
  });
});
