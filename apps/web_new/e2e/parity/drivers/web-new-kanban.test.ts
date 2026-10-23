// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver board methods (NEWFRONT-622, ISS-028–035).
// A stub Page stands in for Playwright; every assertion is about the marker
// a method reads or the control it activates, not about a browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  press?: (key: string) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  inputValue?: () => Promise<string>;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByLabel?: (label: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    inputValue: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    nth: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByLabel: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByTestId: () => locator(),
    getByRole: () => locator(),
    locator: () => locator(),
    waitForTimeout: async () => undefined,
    viewportSize: () => ({ width: 1280, height: 720 }),
    ...overrides,
  } as unknown as Page;
}

/** A column/lane/card element with fixed markers and children. */
function element(
  attributes: Record<string, string | null>,
  children: Record<string, Locator> = {},
  text = ""
): Locator {
  return locator({
    getAttribute: async (name: string) => attributes[name] ?? null,
    innerText: async () => text,
    getByTestId: (id: string) => children[id] ?? locator({ count: async () => 0 }),
    getByRole: () => locator({ count: async () => 0 }),
  });
}

/** A collection that returns the same node for whichever index is asked. */
function list(nodes: Locator[]): Locator {
  return locator({ count: async () => nodes.length, nth: (index: number) => nodes[index]! });
}

describe("kanbanOpenBoard", () => {
  it("returns without clicking when the board already renders", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "layout-switcher-segmented"
          ? locator({ getByRole: () => locator({ first: () => locator({ waitFor: async () => undefined }) }) })
          : locator({ count: async () => 1, isVisible: async () => true, click }),
      locator: () => locator(),
    });
    await new WebNewDriver(page).kanbanOpenBoard();
    expect(click).not.toHaveBeenCalled();
  });

  it("clicks the board segment and waits for the board container", async () => {
    const clicks: string[] = [];
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "layout-switcher-segmented") {
          return locator({ getByRole: () => locator({ first: () => locator({ waitFor: async () => undefined }) }) });
        }
        return locator({
          count: async () => (clicks.length > 0 ? 1 : 0),
          isVisible: async () => clicks.length > 0,
          waitFor: async () => undefined,
        });
      },
      locator: (selector: string) =>
        selector === '[data-layout="kanban"]'
          ? locator({ isVisible: async () => true, click: async () => void clicks.push("board") })
          : locator({ count: async () => 0, isVisible: async () => false }),
    });
    await new WebNewDriver(page).kanbanOpenBoard();
    expect(clicks).toEqual(["board"]);
  });

  it("uses the compact dropdown's Board entry", async () => {
    const picks: string[] = [];
    let opened = false;
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "layout-switcher-segmented") {
          return locator({ getByRole: () => locator({ first: () => locator({ waitFor: async () => undefined }) }) });
        }
        if (id === "layout-switcher-dropdown") {
          return locator({
            getByRole: () =>
              locator({
                first: () =>
                  locator({
                    count: async () => 1,
                    isVisible: async () => true,
                    click: async () => void (opened = true),
                  }),
              }),
          });
        }
        return locator({ count: async () => (opened ? 1 : 0), isVisible: async () => opened });
      },
      locator: (selector: string) => {
        if (selector === '[data-layout="kanban"]') return locator({ isVisible: async () => false });
        if (selector === '[role="menuitem"], [role="menuitemradio"]') {
          return locator({ count: async () => 1, isVisible: async () => opened });
        }
        return locator();
      },
      getByRole: (_role: string, options?: { name?: string }) => {
        if (options?.name === "Board") {
          return locator({
            first: () => locator({ click: async () => void picks.push("Board") }),
          });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).kanbanOpenBoard();
    expect(picks).toEqual(["Board"]);
  });
});

describe("kanbanBoardVisible", () => {
  it("reports the board container's visibility", async () => {
    await expect(new WebNewDriver(pageOf()).kanbanBoardVisible()).resolves.toBe(true);
    const hidden = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(hidden).kanbanBoardVisible()).resolves.toBe(false);
  });
});

function boardPage(options: {
  columns?: Locator;
  lanes?: Locator;
  cards?: Locator;
  columnsByName?: Record<string, Locator>;
  peekTitle?: string;
}) {
  return pageOf({
    getByTestId: (id: string) => {
      if (id === "kanban-column") return options.columns ?? list([]);
      if (id === "kanban-lane") return options.lanes ?? list([]);
      if (id === "kanban-card") return options.cards ?? list([]);
      if (id === "kanban-group-header") return list([]);
      if (id === "issue-preview") {
        return locator({ getByLabel: () => locator({ inputValue: async () => options.peekTitle ?? "" }) });
      }
      return locator();
    },
  });
}

describe("kanbanColumns", () => {
  it("reads each flat column's key, name and live count", async () => {
    // Flat columns carry no labeled header children: the name rides the
    // column's own marker and the count the header's count marker, exactly
    // as KanbanColumn renders them.
    const todo = element(
      { "data-column-key": "todo", "data-column-name": "Todo", "data-collapsed": null },
      {
        "kanban-column-count": locator({ innerText: async () => "2" }),
      }
    );
    const done = element(
      { "data-column-key": "done", "data-column-name": "Done", "data-collapsed": "true" },
      {
        "kanban-column-count": locator({ innerText: async () => "0" }),
      }
    );
    const page = boardPage({ columns: list([todo, done]) });
    await expect(new WebNewDriver(page).kanbanColumns()).resolves.toEqual([
      { id: "todo", name: "Todo", count: 2, rendered: true },
      { id: "done", name: "Done", count: 0, rendered: true },
    ]);
  });

  it("reads the swimlane group header row when sub-grouped", async () => {
    const header = element(
      { "data-column-key": "todo" },
      {
        "kanban-group-name": locator({ getAttribute: async () => "Todo" }),
        "kanban-group-count": locator({ innerText: async () => "3" }),
      }
    );
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "kanban-group-header" ? list([header]) : id === "kanban-column" ? list([]) : locator(),
    });
    await expect(new WebNewDriver(page).kanbanColumns()).resolves.toEqual([
      { id: "todo", name: "Todo", count: 3, rendered: true },
    ]);
  });
});

describe("kanbanSwimlanes", () => {
  it("reads lane names, cumulative counts and data keys", async () => {
    const bug = element(
      { "data-lane-key": "bug" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => "Bug" }),
        "kanban-lane-count": locator({ innerText: async () => "2" }),
      }
    );
    const none = element(
      { "data-lane-key": "None" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => "None" }),
        "kanban-lane-count": locator({ innerText: async () => " 1\n" }),
      }
    );
    const page = boardPage({ lanes: list([bug, none]) });
    await expect(new WebNewDriver(page).kanbanSwimlanes()).resolves.toEqual([
      { id: "bug", name: "Bug", count: 2, rendered: true },
      { id: "None", name: "None", count: 1, rendered: true },
    ]);
  });
});

describe("header fallbacks", () => {
  // Title/inner-text fallbacks belong to the labeled header children the
  // swimlane group headers render; flat columns read the column markers.
  function groupHeaderPage(header: Locator) {
    return pageOf({
      getByTestId: (id: string) =>
        id === "kanban-group-header" ? list([header]) : id === "kanban-column" ? list([]) : locator(),
    });
  }

  it("falls back to inner text when a header carries no title attribute", async () => {
    const header = element(
      { "data-column-key": "todo" },
      {
        "kanban-group-name": locator({
          getAttribute: async () => null,
          innerText: async () => "  Todo   list \n",
        }),
        "kanban-group-count": locator({ innerText: async () => "" }),
      }
    );
    const page = groupHeaderPage(header);
    await expect(new WebNewDriver(page).kanbanColumns()).resolves.toEqual([
      { id: "todo", name: "Todo list", count: 0, rendered: true },
    ]);
  });

  it("keeps the header text when no name or title reads back", async () => {
    const header = element(
      {},
      {
        "kanban-group-name": locator({
          getAttribute: async () => null,
          innerText: async () => null,
        }),
        "kanban-group-count": locator({ innerText: async () => "7" }),
      }
    );
    const page = groupHeaderPage(header);
    await expect(new WebNewDriver(page).kanbanColumns()).resolves.toEqual([
      { id: "", name: "", count: 7, rendered: true },
    ]);
  });

  it("reads a flat column's markers as rendered, ignoring group children", async () => {
    // A flat column never renders group-header children; the reader must
    // not consult them even when stray markers exist.
    const column = element(
      { "data-column-key": "todo", "data-column-name": "  Todo \n" },
      {
        "kanban-column-count": locator({ innerText: async () => "4" }),
        "kanban-group-name": locator({ getAttribute: async () => "Decoy" }),
        "kanban-group-count": locator({ innerText: async () => "9" }),
      }
    );
    const page = boardPage({ columns: list([column]) });
    await expect(new WebNewDriver(page).kanbanColumns()).resolves.toEqual([
      { id: "todo", name: "Todo", count: 4, rendered: true },
    ]);
  });
});

describe("lane lookups", () => {
  it("falls back to the lane's inner text when it carries no title", async () => {
    const lane = element(
      { "data-lane-key": "bug" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => null, innerText: async () => " Bug\n" }),
        "kanban-lane-count": locator({ innerText: async () => "1" }),
      }
    );
    const page = boardPage({ lanes: list([lane]) });
    await expect(new WebNewDriver(page).kanbanSwimlanes()).resolves.toEqual([
      { id: "bug", name: "Bug", count: 1, rendered: true },
    ]);
  });

  it("walks past decoy lanes in server order", async () => {
    const bug = element(
      { "data-lane-key": "bug" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => "Bug" }),
        "kanban-lane-count": locator({ innerText: async () => "1" }),
      }
    );
    const none = element(
      { "data-lane-key": "None" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => "None" }),
        "kanban-lane-count": locator({ innerText: async () => "0" }),
      }
    );
    const page = boardPage({ lanes: list([none, bug]) });
    await expect(new WebNewDriver(page).kanbanSwimlanes()).resolves.toEqual([
      { id: "None", name: "None", count: 0, rendered: true },
      { id: "bug", name: "Bug", count: 1, rendered: true },
    ]);
  });

  it("throws for an unknown swimlane", async () => {
    await expect(new WebNewDriver(boardPage({})).kanbanToggleSwimlane("Nope")).rejects.toThrow(/no kanban swimlane/);
  });

  it("swallows attribute read failures on header and card markers", async () => {
    const column = element(
      {},
      {
        "kanban-column-count": locator({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
      }
    );
    column.getAttribute = (async () => {
      throw new Error("detached");
    }) as Locator["getAttribute"];
    const card = element({}, { "kanban-card-name": locator({ innerText: async () => "Fix login" }) });
    card.getAttribute = (async () => {
      throw new Error("detached");
    }) as Locator["getAttribute"];
    const page = boardPage({ columns: list([column]), cards: list([card]) });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanColumns()).resolves.toEqual([{ id: "", name: "", count: 0, rendered: true }]);
    await expect(driver.kanbanCards()).resolves.toEqual([
      { issueId: "", name: "Fix login", groupId: "", subGroupId: "" },
    ]);
  });
});

describe("kanbanCards", () => {
  it("reads the issue id, name and both group keys", async () => {
    const card = element(
      { "data-issue-id": "issue-1", "data-group-key": "todo", "data-lane-key": "bug" },
      { "kanban-card-name": locator({ innerText: async () => "  Fix login \n" }) }
    );
    const page = boardPage({ cards: list([card]) });
    await expect(new WebNewDriver(page).kanbanCards()).resolves.toEqual([
      { issueId: "issue-1", name: "Fix login", groupId: "todo", subGroupId: "bug" },
    ]);
  });
});

describe("kanbanColumnCards", () => {
  it("reads a named column's card titles in order", async () => {
    const column = element(
      { "data-column-name": "Todo" },
      {
        "kanban-card-name": list([
          locator({ innerText: async () => "Fix login" }),
          locator({ innerText: async () => "Ship it" }),
        ]),
      }
    );
    const decoy = element({ "data-column-name": "Done" });
    const noName = element({});
    const page = boardPage({ columns: list([decoy, noName, column]) });
    await expect(new WebNewDriver(page).kanbanColumnCards("Todo")).resolves.toEqual(["Fix login", "Ship it"]);
  });

  it("throws for an unknown column", async () => {
    await expect(new WebNewDriver(boardPage({})).kanbanColumnCards("Nope")).rejects.toThrow(/no kanban column/);
  });
});

describe("column and lane collapse", () => {
  it("clicks the matching fold control and reports the new state", async () => {
    let collapsed = false;
    const clicks: string[] = [];
    const name = "Todo";
    const column = locator({
      getAttribute: async (attribute: string) => {
        if (attribute === "data-column-name") return name;
        if (attribute === "data-collapsed") return collapsed ? "true" : null;
        return null;
      },
      getByRole: (_role: string, options?: { name?: string }) =>
        locator({
          first: () =>
            locator({
              click: async () => {
                clicks.push(options?.name ?? "");
                collapsed = true;
              },
            }),
        }),
    });
    const page = boardPage({ columns: list([column]) });
    const driver = new WebNewDriver(page);
    await driver.kanbanToggleColumn(name);
    expect(clicks).toEqual(["Collapse Todo column"]);
    await expect(driver.kanbanColumnCollapsed(name)).resolves.toBe(true);
  });

  it("offers the expand control for a folded column", async () => {
    const names: string[] = [];
    let collapsed: string | null = "true";
    const column = locator({
      getAttribute: async (attribute: string) =>
        attribute === "data-column-name" ? "Todo" : attribute === "data-collapsed" ? collapsed : null,
      getByRole: (_role: string, options?: { name?: string }) =>
        locator({
          first: () =>
            locator({
              click: async () => {
                names.push(options?.name ?? "");
                collapsed = null;
              },
            }),
        }),
    });
    const page = boardPage({ columns: list([column]) });
    await new WebNewDriver(page).kanbanToggleColumn("Todo");
    expect(names).toEqual(["Expand Todo column"]);
  });

  it("reports a missing collapse marker as expanded", async () => {
    const column = locator({
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
    });
    const page = boardPage({ columns: list([column]) });
    await expect(new WebNewDriver(page).kanbanColumnCollapsed("Todo")).resolves.toBe(false);
  });

  it("reports a missing aria-expanded lane as expanded", async () => {
    const lane = element(
      {},
      {
        "kanban-lane-name": locator({ getAttribute: async () => "Bug" }),
        "kanban-lane-header": locator({ getAttribute: async () => null }),
      }
    );
    const page = boardPage({ lanes: list([lane]) });
    await expect(new WebNewDriver(page).kanbanSwimlaneCollapsed("Bug")).resolves.toBe(false);
  });

  it("toggles a lane through its header and reads aria-expanded", async () => {
    let expanded = "true";
    const header = locator({
      getAttribute: async (attribute: string) => (attribute === "aria-expanded" ? expanded : null),
      click: async () => void (expanded = "false"),
    });
    const lane = element(
      { "data-lane-key": "bug" },
      {
        "kanban-lane-name": locator({ getAttribute: async () => "Bug" }),
        "kanban-lane-header": header,
      }
    );
    const page = boardPage({ lanes: list([lane]) });
    const driver = new WebNewDriver(page);
    await driver.kanbanToggleSwimlane("Bug");
    await expect(driver.kanbanSwimlaneCollapsed("Bug")).resolves.toBe(true);
  });
});

describe("card contents and open", () => {
  function cardPage(
    name: string,
    overrides: { href?: string | null; quickActions?: boolean; peekTitle?: string } = {}
  ) {
    const card = element(
      { "data-issue-id": "issue-1" },
      {
        "kanban-card-name": locator({ innerText: async () => name }),
        "kanban-card-link": locator({
          innerText: async () => `PAR-12\n${name}`,
          getAttribute: async (attribute: string) =>
            attribute === "href" ? (overrides.href ?? "/acme/browse/PAR-12") : null,
        }),
        "kanban-card-properties": locator({ getByTestId: () => locator({ count: async () => 1 }) }),
      }
    );
    const actionButton = locator({
      count: async () => (overrides.quickActions === false ? 0 : 1),
      isVisible: async () => overrides.quickActions !== false,
    });
    const hover = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list([card]);
        if (id === "kanban-column") return list([]);
        if (id === "issue-preview") {
          return locator({ getByLabel: () => locator({ inputValue: async () => overrides.peekTitle ?? "" }) });
        }
        return locator();
      },
    });
    card.getByTestId = ((id: string) =>
      id === "kanban-card-name"
        ? locator({ innerText: async () => name, hover })
        : locator({
            innerText: async () => `PAR-12\n${name}`,
            getAttribute: async (attribute: string) =>
              attribute === "href" ? (overrides.href ?? "/acme/browse/PAR-12") : null,
            click: async () => undefined,
          })) as Locator["getByTestId"];
    card.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "button" && options?.name === `Actions for ${name}`) return actionButton;
      return locator({ count: async () => 0 });
    }) as Locator["getByRole"];
    return { page, hover };
  }

  it("reads the identifier, properties, quick actions and href", async () => {
    const { page, hover } = cardPage("Fix login");
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanCardIdentifier("Fix login")).resolves.toBe("PAR-12");
    await expect(driver.kanbanCardShowsProperties("Fix login")).resolves.toBe(true);
    await expect(driver.kanbanCardQuickActionsVisible("Fix login")).resolves.toBe(true);
    await expect(driver.kanbanCardHref("Fix login")).resolves.toBe("/acme/browse/PAR-12");
    await driver.kanbanCardHover("Fix login");
    expect(hover).toHaveBeenCalled();
  });

  it("reports a hidden identifier and no properties when the card omits them", async () => {
    const bare = element(
      { "data-issue-id": "issue-1" },
      {
        "kanban-card-name": locator({ innerText: async () => "Plain" }),
        "kanban-card-link": locator({ innerText: async () => "Plain", getAttribute: async () => null }),
      }
    );
    const decoy = element({}, { "kanban-card-name": locator({ innerText: async () => "Other" }) });
    const page = pageOf({
      getByTestId: (id: string) => (id === "kanban-card" ? list([decoy, bare]) : locator({ count: async () => 0 })),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanCardIdentifier("Plain")).resolves.toBeNull();
    await expect(driver.kanbanCardShowsProperties("Plain")).resolves.toBe(false);
    await expect(driver.kanbanCardHref("Plain")).resolves.toBeNull();
  });

  it("opens the peek by clicking the card link", async () => {
    const { page } = cardPage("Fix login", { peekTitle: "Fix login" });
    await new WebNewDriver(page).kanbanOpenCardPeek("Fix login");
  });

  it("reports a card with a detached name or link as unnamed", async () => {
    const card = element(
      {},
      {
        "kanban-card-name": locator({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
        "kanban-card-link": locator({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
      }
    );
    const page = pageOf({
      getByTestId: (id: string) => (id === "kanban-card" ? list([card]) : locator({ count: async () => 0 })),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanCards()).resolves.toEqual([{ issueId: "", name: "", groupId: "", subGroupId: "" }]);
    await expect(driver.kanbanCardIdentifier("")).resolves.toBeNull();
  });

  it("throws for a card that is not rendered", async () => {
    await expect(new WebNewDriver(boardPage({})).kanbanCardIdentifier("Nope")).rejects.toThrow(/no kanban card/);
  });
});

describe("kanban quick-add", () => {
  function quickAddPage(options: { present: boolean }) {
    const typed: string[] = [];
    let created: string | null = null;
    const field = locator({
      fill: async (value: string) => void typed.push(value),
      press: async () => {
        created = typed[0] ?? "";
      },
    });
    const column = locator({
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
      getByTestId: (id: string) =>
        id === "kanban-quick-add"
          ? locator({ count: async () => (options.present ? 1 : 0), isVisible: async () => options.present })
          : id === "kanban-card-name"
            ? list(created === null ? [] : [locator({ innerText: async () => created ?? "" })])
            : locator({ count: async () => 0 }),
      getByRole: () => locator({ first: () => locator({ click: async () => undefined }) }),
      getByLabel: () => locator({ first: () => field }),
    });
    return { page: boardPage({ columns: list([column]) }), typed };
  }

  it("reports whether a column ends with the quick-add entry", async () => {
    await expect(new WebNewDriver(quickAddPage({ present: true }).page).kanbanColumnHasQuickAdd("Todo")).resolves.toBe(
      true
    );
    await expect(new WebNewDriver(quickAddPage({ present: false }).page).kanbanColumnHasQuickAdd("Todo")).resolves.toBe(
      false
    );
  });

  it("types a title and submits it with Enter", async () => {
    const { page, typed } = quickAddPage({ present: true });
    await new WebNewDriver(page).kanbanQuickAdd("Todo", "Fresh item");
    expect(typed).toEqual(["Fresh item"]);
  });
});
