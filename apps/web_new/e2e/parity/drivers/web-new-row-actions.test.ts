// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver row-action methods (NEWFRONT-658,
// ISS-060–067, ISS-012). A stub Page records the selectors each method
// reads, so dropping the markup hooks the row-action scenarios depend on
// fails here before the parity stack.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string | null>;
  getAttribute?: (name: string) => Promise<string | null>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    innerText: async () => "",
    getAttribute: async () => null,
    isVisible: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node(),
    getByTestId: () => node(),
    getByRole: () => node(),
    filter: () => node(),
    first: () => self as unknown as Locator,
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
  });
}

interface MenuScript {
  label: string;
  note?: string;
  disabled?: "aria" | "data" | false;
}

function menuItem(entry: MenuScript): Locator {
  return node({
    innerText: async () => entry.label,
    getAttribute: async (name: string) => {
      if (name === "aria-disabled") return entry.disabled === "aria" ? "true" : null;
      if (name === "data-disabled") return entry.disabled === "data" ? "" : null;
      return null;
    },
    locator: (selector: string) => {
      if (selector === '[data-menu-title="true"]') {
        return entry.note === undefined ? node({ count: async () => 0 }) : node({ innerText: async () => entry.label });
      }
      if (selector === '[data-menu-description="true"]') {
        return entry.note === undefined
          ? node({ count: async () => 0 })
          : node({ innerText: async () => entry.note ?? "" });
      }
      return node({ count: async () => 0 });
    },
  });
}

function openMenu(entries: MenuScript[]): Locator {
  return collection(entries.map((entry) => menuItem(entry)));
}

interface PageRoutes {
  locator?: (selector: string) => Locator;
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  placeholder?: (text: string) => Locator;
  keyboardPress?: (key: string) => Promise<void>;
  evaluate?: (fn: (...args: never[]) => unknown) => Promise<unknown>;
  waitForURL?: (fn: (url: URL) => boolean, options?: unknown) => Promise<void>;
  popupUrl?: string;
}

interface PageStub {
  page: Page;
  selectors: string[];
  testIds: string[];
  roles: Array<{ role: string; name: unknown }>;
  keys: string[];
  grants: string[][];
  urlPredicates: Array<(url: URL) => boolean>;
  popupsClosed: number;
}

function pageOf(routes: PageRoutes = {}): PageStub {
  const state: PageStub = {
    page: undefined as unknown as Page,
    selectors: [],
    testIds: [],
    roles: [],
    keys: [],
    grants: [],
    urlPredicates: [],
    popupsClosed: 0,
  };
  state.page = {
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
      grantPermissions: async (permissions: string[]) => {
        state.grants.push(permissions);
      },
      waitForEvent: async () => ({
        url: () => routes.popupUrl ?? "",
        close: async () => {
          state.popupsClosed += 1;
        },
      }),
    }),
    evaluate: (fn: (...args: never[]) => unknown) => {
      if (routes.evaluate !== undefined) return routes.evaluate(fn);
      return Promise.resolve(undefined);
    },
    waitForURL: async (fn: (url: URL) => boolean, options?: unknown) => {
      state.urlPredicates.push(fn);
      await routes.waitForURL?.(fn, options);
    },
    reload: async () => null,
  } as unknown as Page;
  return state;
}

/** A list row with a menu trigger and an anchor. */
function listRow(href: string | null = "/ws/browse/WEB-1/"): Locator {
  return node({
    getByTestId: (id: string) => (id === "row-menu-trigger" ? node() : node({ count: async () => 0 })),
    locator: (selector: string) => {
      if (selector === "a[data-row-anchor]") {
        return href === null ? node({ count: async () => 0 }) : node({ getAttribute: async () => href });
      }
      if (selector === "a") {
        return href === null ? node({ count: async () => 0 }) : node({ getAttribute: async () => href });
      }
      return node({ count: async () => 0 });
    },
  });
}

/** Routes `page.locator` at the row finder to `row` (or nothing). */
function rowFinder(name: string, row: Locator | null): (selector: string) => Locator {
  const wanted = `[data-testid="list-row"]:has(p[title="${name}"]), [data-row-name="${name}"]`;
  return (selector: string) => {
    if (selector === wanted) return row === null ? node({ count: async () => 0 }) : row;
    return node({ count: async () => 0 });
  };
}

const PROJECT_MENU: MenuScript[] = [
  { label: "Edit" },
  { label: "Make a copy" },
  { label: "Open in new tab" },
  { label: "Copy link" },
  { label: "Move to project" },
  { label: "Archive", note: "Only completed or canceled work items can be archived", disabled: "aria" },
  { label: "Delete" },
];

function menuRoutes(entries: MenuScript[]): Pick<PageRoutes, "role"> {
  const menu = openMenu(entries);
  return {
    role: (role: string) => (role === "menuitem" ? menu : node({ count: async () => 0 })),
  };
}

describe("web_new row menus", () => {
  it("reads the row menu titles without their notes and escapes", async () => {
    const row = listRow();
    const stub = pageOf({ locator: rowFinder("Alpha", row), ...menuRoutes(PROJECT_MENU) });
    await expect(new WebNewDriver(stub.page).layoutsRowMenuItems("Alpha")).resolves.toEqual([
      "Edit",
      "Make a copy",
      "Open in new tab",
      "Copy link",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    expect(stub.selectors[0]).toBe('[data-testid="list-row"]:has(p[title="Alpha"]), [data-row-name="Alpha"]');
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("escapes quotes and backslashes in the row finder", async () => {
    const stub = pageOf({ locator: rowFinder('a\\b"c', null), ...menuRoutes([]) });
    await expect(new WebNewDriver(stub.page).layoutsRowMenuItems('a\\b"c')).resolves.toEqual([]);
    expect(stub.selectors[0]).toBe('[data-testid="list-row"]:has(p[title="a\\\\b\\"c"]), [data-row-name="a\\\\b\\"c"]');
  });

  it("hovers the row before opening its menu", async () => {
    let hovers = 0;
    const row = node({
      hover: async () => {
        hovers += 1;
      },
      getByTestId: () => node(),
    });
    const stub = pageOf({ locator: rowFinder("Alpha", row), ...menuRoutes([]) });
    await new WebNewDriver(stub.page).layoutsRowMenuItems("Alpha");
    expect(hovers).toBe(1);
  });

  it("trims the labels it reads", async () => {
    const stub = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "  Edit  " }]),
    });
    await expect(new WebNewDriver(stub.page).layoutsRowMenuItems("Alpha")).resolves.toEqual(["Edit"]);
  });

  it("chooses a menu entry by its title without dismissing first", async () => {
    const clicks: string[] = [];
    const stub = pageOf({
      locator: rowFinder("Alpha", listRow()),
      role: () => {
        const items = PROJECT_MENU.map((entry) =>
          node({
            innerText: async () => entry.label,
            locator: (selector: string) =>
              selector === '[data-menu-title="true"]' && entry.note !== undefined
                ? node({ innerText: async () => entry.label })
                : node({ count: async () => 0 }),
            click: async () => {
              clicks.push(entry.label);
            },
          })
        );
        return collection(items);
      },
    });
    await new WebNewDriver(stub.page).layoutsRowMenuChoose("Alpha", "Archive");
    expect(clicks).toEqual(["Archive"]);
    expect(stub.keys).toEqual([]);
    expect(stub.grants).toEqual([]);
  });

  it("grants clipboard permissions before picking Copy link", async () => {
    const stub = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Copy link" }]),
    });
    await new WebNewDriver(stub.page).layoutsRowMenuChoose("Alpha", "Copy link");
    expect(stub.grants).toEqual([["clipboard-read", "clipboard-write"]]);
  });

  it("throws when the open menu has no such entry", async () => {
    const stub = pageOf({ locator: rowFinder("Alpha", listRow()), ...menuRoutes([{ label: "Edit" }]) });
    await expect(new WebNewDriver(stub.page).layoutsRowMenuChoose("Alpha", "Archive")).rejects.toThrow(
      "[parity] open menu has no Archive."
    );
  });

  it("reads the same entries on right-click", async () => {
    let button: string | undefined;
    const row = node({
      click: async (options?: unknown) => {
        button = (options as { button?: string } | undefined)?.button;
      },
      getByTestId: () => node(),
    });
    const stub = pageOf({ locator: rowFinder("Alpha", row), ...menuRoutes(PROJECT_MENU) });
    await expect(new WebNewDriver(stub.page).layoutsRowContextMenuItems("Alpha")).resolves.toHaveLength(7);
    expect(button).toBe("right");
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("reads the disabled state from either marker", async () => {
    const byAria = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Archive", disabled: "aria" }]),
    });
    await expect(new WebNewDriver(byAria.page).layoutsRowMenuItemDisabled("Alpha", "Archive")).resolves.toBe(true);
    const byData = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Archive", disabled: "data" }]),
    });
    await expect(new WebNewDriver(byData.page).layoutsRowMenuItemDisabled("Alpha", "Archive")).resolves.toBe(true);
    const enabled = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Archive", disabled: false }]),
    });
    await expect(new WebNewDriver(enabled.page).layoutsRowMenuItemDisabled("Alpha", "Archive")).resolves.toBe(false);
    expect(byAria.keys).toEqual(["Escape"]);
  });

  it("reads the item note, null when absent or blank", async () => {
    const noted = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Archive", note: "Only  completed\nor canceled" }]),
    });
    await expect(new WebNewDriver(noted.page).layoutsRowMenuItemNote("Alpha", "Archive")).resolves.toBe(
      "Only completed or canceled"
    );
    const plain = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Edit" }]),
    });
    await expect(new WebNewDriver(plain.page).layoutsRowMenuItemNote("Alpha", "Edit")).resolves.toBeNull();
    const blank = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes([{ label: "Edit", note: "   " }]),
    });
    await expect(new WebNewDriver(blank.page).layoutsRowMenuItemNote("Alpha", "Edit")).resolves.toBeNull();
  });
});

describe("web_new row anchors and peek", () => {
  it("reads the marked row anchor href", async () => {
    const stub = pageOf({ locator: rowFinder("Alpha", listRow("/ws/browse/WEB-9/")) });
    await expect(new WebNewDriver(stub.page).layoutsRowHref("Alpha")).resolves.toBe("/ws/browse/WEB-9/");
  });

  it("falls back to the first link on unmarked rows", async () => {
    const row = node({
      locator: (selector: string) =>
        selector === "a[data-row-anchor]"
          ? node({ count: async () => 0 })
          : node({ getAttribute: async () => "/ws/browse/WEB-3/" }),
    });
    const stub = pageOf({ locator: rowFinder("Alpha", row) });
    await expect(new WebNewDriver(stub.page).layoutsRowHref("Alpha")).resolves.toBe("/ws/browse/WEB-3/");
  });

  it("reports null when the row carries no link", async () => {
    const row = node({ locator: () => node({ count: async () => 0 }) });
    const stub = pageOf({ locator: rowFinder("Alpha", row) });
    await expect(new WebNewDriver(stub.page).layoutsRowHref("Alpha")).resolves.toBeNull();
  });

  it("opens the peek through the anchor and waits for the panel", async () => {
    let clicked = 0;
    const row = node({
      locator: () => node({ click: async () => void (clicked += 1) }),
    });
    const stub = pageOf({
      locator: rowFinder("Alpha", row),
      testId: (id: string) => (id === "issue-preview" ? node() : node({ count: async () => 0 })),
    });
    await new WebNewDriver(stub.page).layoutsRowOpenPeek("Alpha");
    expect(clicked).toBe(1);
    expect(stub.urlPredicates).toHaveLength(1);
    const settled = stub.urlPredicates[0] ?? (() => false);
    expect(settled(new URL("https://app.test/ws/list?peekIssueId=1"))).toBe(true);
    expect(settled(new URL("https://app.test/ws/list"))).toBe(false);
    expect(stub.testIds).toContain("issue-preview");
  });

  it("reads the new-tab URL from its popup and closes it", async () => {
    const stub = pageOf({
      locator: rowFinder("Alpha", listRow()),
      ...menuRoutes(PROJECT_MENU),
      popupUrl: "https://app.test/ws/browse/WEB-1/",
    });
    await expect(new WebNewDriver(stub.page).layoutsRowMenuOpenNewTabUrl("Alpha")).resolves.toBe(
      "https://app.test/ws/browse/WEB-1/"
    );
    expect(stub.popupsClosed).toBe(1);
  });
});

describe("web_new lifecycle modals", () => {
  function dialogRoutes(name: string, dialog: Locator): Pick<PageRoutes, "role"> {
    return {
      role: (role: string, options?: unknown) => {
        if (role === "dialog" && (options as { name?: unknown } | undefined)?.name === name) return dialog;
        return node({ count: async () => 0 });
      },
    };
  }

  it("reads delete visibility and confirms through its button", async () => {
    const shown = pageOf(dialogRoutes("Delete work item", node()));
    await expect(new WebNewDriver(shown.page).layoutsDeleteModalVisible()).resolves.toBe(true);
    const hidden = pageOf(dialogRoutes("Delete work item", node({ count: async () => 0 })));
    await expect(new WebNewDriver(hidden.page).layoutsDeleteModalVisible()).resolves.toBe(false);

    let confirmed: unknown;
    const states: unknown[] = [];
    const dialog = node({
      waitFor: async (options?: unknown) => {
        states.push((options as { state?: unknown } | undefined)?.state);
      },
      getByRole: (role: string, options?: unknown) => {
        confirmed = options;
        return role === "button" ? node() : node({ count: async () => 0 });
      },
    });
    const stub = pageOf(dialogRoutes("Delete work item", dialog));
    await new WebNewDriver(stub.page).layoutsDeleteModalConfirm();
    expect(confirmed).toEqual({ name: "Delete", exact: true });
    expect(states).toEqual(["visible", "hidden"]);
  });

  it("matches the archive dialog on its heading prefix", async () => {
    const filters: unknown[] = [];
    const dialog = node({
      filter: (options: unknown) => {
        filters.push(options);
        return node();
      },
      getByRole: () => node(),
    });
    const stub = pageOf({ role: () => dialog });
    await expect(new WebNewDriver(stub.page).layoutsArchiveModalVisible()).resolves.toBe(true);
    expect(stub.roles).toContainEqual({ role: "heading", name: expect.any(RegExp) });
    const heading = stub.roles.find((entry) => entry.role === "heading")?.name as RegExp;
    expect("Archive work item WEB 42").toMatch(heading);
    expect("Delete work item").not.toMatch(heading);
    expect(filters).toHaveLength(1);

    let confirmed: unknown;
    const states: unknown[] = [];
    const filtered = node({
      waitFor: async (options?: unknown) => {
        states.push((options as { state?: unknown } | undefined)?.state);
      },
      getByRole: (role: string, options?: unknown) => {
        if (role === "button") confirmed = options;
        return node();
      },
    });
    const confirmDialog = node({
      filter: () => filtered,
      getByRole: () => node(),
    });
    const confirmStub = pageOf({ role: () => confirmDialog });
    await new WebNewDriver(confirmStub.page).layoutsArchiveModalConfirm();
    expect(confirmed).toEqual({ name: "Archive", exact: true });
    expect(states).toEqual(["visible", "hidden"]);
  });

  it("reads move visibility and picks the named target", async () => {
    const shown = pageOf(dialogRoutes("Move to project", node()));
    await expect(new WebNewDriver(shown.page).layoutsMoveModalVisible()).resolves.toBe(true);
    const hidden = pageOf(dialogRoutes("Move to project", node({ count: async () => 0 })));
    await expect(new WebNewDriver(hidden.page).layoutsMoveModalVisible()).resolves.toBe(false);

    const filters: unknown[] = [];
    const states: unknown[] = [];
    let picked = 0;
    const dialog = node({
      waitFor: async (options?: unknown) => {
        states.push((options as { state?: unknown } | undefined)?.state);
      },
      getByTestId: (id: string) =>
        id === "move-target"
          ? node({
              filter: (options: unknown) => {
                filters.push(options);
                return node({ first: () => node({ click: async () => void (picked += 1) }) });
              },
            })
          : node({ count: async () => 0 }),
    });
    const stub = pageOf(dialogRoutes("Move to project", dialog));
    await new WebNewDriver(stub.page).layoutsMoveModalChoose("Parity Move");
    expect(filters).toEqual([{ hasText: "Parity Move" }]);
    expect(picked).toBe(1);
    expect(states).toEqual(["visible", "hidden"]);
  });
});

describe("web_new header and page menus", () => {
  it("reads the detail menu through its trigger", async () => {
    const stub = pageOf({
      testId: (id: string) =>
        id === "detail-menu-trigger" ? node({ first: () => node() }) : node({ count: async () => 0 }),
      ...menuRoutes([
        { label: "Make a copy" },
        { label: "Open in new tab" },
        { label: "Move to project" },
        { label: "Archive" },
        { label: "Delete" },
      ]),
    });
    await expect(new WebNewDriver(stub.page).layoutsDetailMenuItems()).resolves.toEqual([
      "Make a copy",
      "Open in new tab",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    expect(stub.testIds).toContain("detail-menu-trigger");
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("scopes the peek copy control to the peek panel", async () => {
    const missing = pageOf({ testId: () => node({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing.page).layoutsPeekCopyLinkVisible()).resolves.toBe(false);

    const shown = pageOf({
      testId: (id: string) =>
        id === "issue-preview"
          ? node({
              first: () => node({ getByRole: () => node() }),
            })
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(shown.page).layoutsPeekCopyLinkVisible()).resolves.toBe(true);

    const hidden = pageOf({
      testId: (id: string) =>
        id === "issue-preview"
          ? node({
              first: () => node({ getByRole: () => node({ count: async () => 0 }) }),
            })
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(hidden.page).layoutsPeekCopyLinkVisible()).resolves.toBe(false);
  });

  it("reads and chooses the whole-list menu", async () => {
    const stub = pageOf({
      testId: (id: string) => (id === "list-page-actions" ? node() : node({ count: async () => 0 })),
      ...menuRoutes([{ label: "Open in new tab" }, { label: "Copy link" }]),
    });
    await expect(new WebNewDriver(stub.page).layoutsListPageMenuItems()).resolves.toEqual([
      "Open in new tab",
      "Copy link",
    ]);
    expect(stub.keys).toEqual(["Escape"]);

    const clicks: string[] = [];
    const choose = pageOf({
      testId: (id: string) => (id === "list-page-actions" ? node() : node({ count: async () => 0 })),
      role: () =>
        collection(
          ["Open in new tab", "Copy link"].map((label) =>
            node({
              innerText: async () => label,
              locator: () => node({ count: async () => 0 }),
              click: async () => void clicks.push(label),
            })
          )
        ),
    });
    await new WebNewDriver(choose.page).layoutsListPageMenuChoose("Copy link");
    expect(clicks).toEqual(["Copy link"]);
    expect(choose.grants).toEqual([["clipboard-read", "clipboard-write"]]);

    const plain = pageOf({
      testId: (id: string) => (id === "list-page-actions" ? node() : node({ count: async () => 0 })),
      ...menuRoutes([{ label: "Open in new tab" }]),
    });
    await new WebNewDriver(plain.page).layoutsListPageMenuChoose("Open in new tab");
    expect(plain.grants).toEqual([]);
  });

  /** A group header whose plus is a menu trigger (span) or a direct div. */
  function groupHeader(title: string, menu: boolean): Locator {
    const labelled = `Add to ${title}`;
    return node({
      locator: (selector: string) => {
        if (selector === `span[aria-label="${labelled}"]`) {
          return menu ? node() : node({ count: async () => 0 });
        }
        if (selector === `div[aria-label="${labelled}"]`) {
          return menu ? node({ count: async () => 0 }) : node();
        }
        return node({ count: async () => 0 });
      },
    });
  }

  function groupHeaders(header: Locator): (selector: string) => Locator {
    return (selector: string) =>
      selector === '[data-testid="list-group-header"]'
        ? node({ filter: () => node({ first: () => header }) })
        : node({ count: async () => 0 });
  }

  it("reads the cycle add menu and null on plain projects", async () => {
    const menu = pageOf({
      locator: groupHeaders(groupHeader("Cycle", true)),
      ...menuRoutes([{ label: "Create work item" }, { label: "Add an existing work item" }]),
    });
    await expect(new WebNewDriver(menu.page).layoutsGroupHeaderAddMenu("Cycle")).resolves.toEqual([
      "Create work item",
      "Add an existing work item",
    ]);
    expect(menu.keys).toEqual(["Escape"]);

    const direct = pageOf({
      locator: groupHeaders(groupHeader("All work items", false)),
      ...menuRoutes([]),
    });
    await expect(new WebNewDriver(direct.page).layoutsGroupHeaderAddMenu("All work items")).resolves.toBeNull();
    expect(direct.roles).toEqual([]);
  });

  it("adds through the header menu, directly, or on a null pick", async () => {
    let menuClicks = 0;
    const header = node({
      locator: (selector: string) =>
        selector.startsWith("span")
          ? node({ click: async () => void (menuClicks += 1) })
          : node({ count: async () => 0 }),
    });
    const clicks: string[] = [];
    const menu = pageOf({
      locator: groupHeaders(header),
      role: () =>
        collection(
          ["Create work item"].map((label) =>
            node({
              innerText: async () => label,
              locator: () => node({ count: async () => 0 }),
              click: async () => void clicks.push(label),
            })
          )
        ),
    });
    await new WebNewDriver(menu.page).layoutsGroupHeaderAddChoose("Cycle", "Create work item");
    expect(menuClicks).toBe(1);
    expect(clicks).toEqual(["Create work item"]);

    let directClicks = 0;
    const plain = node({
      locator: (selector: string) =>
        selector.startsWith("div")
          ? node({ click: async () => void (directClicks += 1) })
          : node({ count: async () => 0 }),
    });
    const direct = pageOf({ locator: groupHeaders(plain), ...menuRoutes([]) });
    await new WebNewDriver(direct.page).layoutsGroupHeaderAddChoose("All work items", "Create work item");
    expect(directClicks).toBe(1);
    expect(direct.roles.map((entry) => entry.role)).toEqual(["dialog"]);

    let nullClicks = 0;
    const nullHeader = node({
      locator: (selector: string) =>
        selector.startsWith("span")
          ? node({ click: async () => void (nullClicks += 1) })
          : node({ count: async () => 0 }),
    });
    const nulled = pageOf({ locator: groupHeaders(nullHeader), ...menuRoutes([]) });
    await new WebNewDriver(nulled.page).layoutsGroupHeaderAddChoose("Cycle", null);
    expect(nullClicks).toBe(1);
    expect(nulled.roles.map((entry) => entry.role)).toEqual(["dialog"]);
  });

  it("settles on the add-existing dialog after that pick", async () => {
    const waits: unknown[] = [];
    const header = node({
      locator: (selector: string) => (selector.startsWith("span") ? node() : node({ count: async () => 0 })),
    });
    const stub = pageOf({
      locator: groupHeaders(header),
      role: (role: string, options?: unknown) => {
        if (role === "menuitem") return openMenu([{ label: "Add an existing work item" }]);
        if (role === "button") {
          waits.push((options as { name?: unknown } | undefined)?.name);
          return node();
        }
        return node({ count: async () => 0 });
      },
    });
    await new WebNewDriver(stub.page).layoutsGroupHeaderAddChoose("Cycle", "Add an existing work item");
    expect(waits).toEqual(["Add selected work items"]);
  });
});

describe("web_new clipboard reads", () => {
  it("grants clipboard permissions before reading", async () => {
    vi.stubGlobal("navigator", { clipboard: { readText: async () => "/ws/browse/WEB-1/" } });
    try {
      const stub = pageOf({ evaluate: (fn) => (fn as () => Promise<string>)() });
      await expect(new WebNewDriver(stub.page).rulesReadClipboard()).resolves.toBe("/ws/browse/WEB-1/");
      expect(stub.grants).toEqual([["clipboard-read", "clipboard-write"]]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("web_new project list branches", () => {
  it("reloads onto the profile tab when it renders", async () => {
    const stub = pageOf({ testId: () => node() });
    await new WebNewDriver(stub.page).layoutsReloadIssues();
    expect(stub.testIds).toEqual(["profile-list", "profile-issues-tab"]);
    expect(stub.selectors).toContain('[data-testid="profile-list"], [data-testid="profile-board"]');
  });

  it("reloads onto a project list without the profile waits", async () => {
    const stub = pageOf({ testId: () => node({ count: async () => 0 }) });
    await new WebNewDriver(stub.page).layoutsReloadIssues();
    expect(stub.testIds).toEqual(["profile-list"]);
    expect(stub.selectors[0]).toContain('[data-testid="list-group-header"]');
  });

  it("reads profile groups from their markers", async () => {
    const groups = collection([
      node({ getAttribute: async () => "Mine" }),
      node({ getAttribute: async () => "  " }),
      node({ getAttribute: async () => null }),
    ]);
    const stub = pageOf({
      testId: () => node(),
      locator: (selector: string) =>
        selector === '[data-testid="profile-list"] [data-profile-group]' ? groups : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsListGroups()).resolves.toEqual(["Mine"]);
  });

  it("reads project groups from the header titles", async () => {
    const titles = ["All work items", "", "Triage"];
    const headers = collection(titles.map((title) => node({ locator: () => node({ innerText: async () => title }) })));
    const stub = pageOf({
      testId: () => node({ count: async () => 0 }),
      locator: (selector: string) =>
        selector === '[data-testid="list-group-header"]' ? headers : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsListGroups()).resolves.toEqual(["All work items", "Triage"]);
  });

  it("reads project group names from the row title attributes", async () => {
    const names = ["Alpha", "", "Beta"];
    const rows = collection(
      names.map((name) =>
        node({
          locator: (selector: string) =>
            selector === "p[title]"
              ? node({
                  first: () => node({ getAttribute: async () => (name === "" ? null : name) }),
                })
              : node({ count: async () => 0 }),
        })
      )
    );
    const section = node({ locator: () => rows });
    const stub = pageOf({
      testId: () => node({ count: async () => 0 }),
      locator: (selector: string) =>
        selector === "section" ? node({ first: () => section }) : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsListGroupIssueNames("All work items")).resolves.toEqual([
      "Alpha",
      "Beta",
    ]);
  });

  it("keeps the profile group names on profile pages", async () => {
    const rows = collection([
      node({ locator: () => node({ nth: () => node({ innerText: async () => "Alpha" }) }) }),
      node({ locator: () => node({ nth: () => node({ innerText: async () => "" }) }) }),
    ]);
    const group = node({ locator: () => rows });
    const stub = pageOf({
      testId: () => node(),
      locator: () => node({ first: () => group }),
    });
    await expect(new WebNewDriver(stub.page).layoutsListGroupIssueNames("Mine")).resolves.toEqual(["Alpha"]);
  });
});
