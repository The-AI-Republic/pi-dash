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
  const empty = locator({ count: async () => 0, isVisible: async () => false });
  return locator({
    count: async () => nodes.length,
    first: () => (nodes.length > 0 ? nodes[0]! : empty),
    nth: (index: number) => nodes[index]!,
  });
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

function namedCard(name: string): Locator {
  return element(
    { "data-issue-id": `issue-${name}` },
    { "kanban-card-name": locator({ innerText: async () => name }) }
  );
}

function placeholderCard(): Locator {
  return element({ "data-issue-id": "issue-far", "data-virtualized": "true" });
}

describe("kanbanColumnCards", () => {
  it("reads a named column's card titles in order", async () => {
    const column = element(
      { "data-column-name": "Todo" },
      { "kanban-card": list([namedCard("Fix login"), namedCard("Ship it")]) }
    );
    const decoy = element({ "data-column-name": "Done" });
    const noName = element({});
    const page = boardPage({ columns: list([decoy, noName, column]) });
    await expect(new WebNewDriver(page).kanbanColumnCards("Todo")).resolves.toEqual(["Fix login", "Ship it"]);
  });

  it("reads virtualized placeholders as empty names", async () => {
    const column = element(
      { "data-column-name": "Todo" },
      { "kanban-card": list([namedCard("Fix login"), placeholderCard()]) }
    );
    const page = boardPage({ columns: list([column]) });
    await expect(new WebNewDriver(page).kanbanColumnCards("Todo")).resolves.toEqual(["Fix login", ""]);
  });

  it("reads an unreadable name as empty", async () => {
    const card = element(
      { "data-issue-id": "issue-1" },
      {
        "kanban-card-name": locator({
          innerText: async () => {
            throw new Error("gone");
          },
        }),
      }
    );
    const column = element({ "data-column-name": "Todo" }, { "kanban-card": list([card]) });
    const page = boardPage({ columns: list([column]) });
    await expect(new WebNewDriver(page).kanbanColumnCards("Todo")).resolves.toEqual([""]);
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
          : id === "kanban-card"
            ? list(created === null ? [] : [namedCard(created ?? "")])
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

function mouseStub(): {
  mouse: { move: (...args: unknown[]) => Promise<void>; down: () => Promise<void>; up: () => Promise<void> };
  moves: Array<{ x: number; y: number; steps?: number }>;
  events: string[];
} {
  const moves: Array<{ x: number; y: number; steps?: number }> = [];
  const events: string[] = [];
  return {
    mouse: {
      move: async (x: unknown, y: unknown, options?: { steps?: number }) => {
        moves.push({ x: x as number, y: y as number, steps: options?.steps });
        events.push("move");
      },
      down: async () => void events.push("down"),
      up: async () => void events.push("up"),
    },
    moves,
    events,
  };
}

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

function dragCard(name: string, rect: Box | null, nameRect: Box | null = null): Locator {
  return locator({
    boundingBox: async () => rect,
    getByTestId: (id: string) =>
      id === "kanban-card-name"
        ? locator({ innerText: async () => name, boundingBox: async () => nameRect })
        : locator({ count: async () => 0 }),
  });
}

function groupHeaderStub(name: string, key: string | null, title: string | null = null): Locator {
  return locator({
    getAttribute: async (attribute: string) => (attribute === "data-column-key" ? key : null),
    getByTestId: (id: string) => {
      if (id !== "kanban-group-name") return locator({ count: async () => 0 });
      return locator({
        getAttribute: async () => title,
        innerText: async () => (title === null ? name : ""),
      });
    },
  });
}

describe("kanban header create", () => {
  it("reads the add control on flat columns", async () => {
    const clicks: string[] = [];
    const column = locator({
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
      getByTestId: (id: string) =>
        id === "kanban-header-create"
          ? locator({
              isVisible: async () => true,
              click: async () => void clicks.push("create"),
            })
          : locator({ count: async () => 0 }),
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "kanban-column" ? list([column]) : list([])) });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanHeaderCreateVisible("Todo")).resolves.toBe(true);
    await driver.kanbanHeaderCreate("Todo");
    expect(clicks).toEqual(["create"]);
  });

  it("reads the add control on swimlane group headers", async () => {
    const header = locator({
      getAttribute: async () => null,
      getByTestId: (id: string) => {
        if (id === "kanban-group-name") return locator({ getAttribute: async () => "Todo" });
        if (id === "kanban-header-create") return locator({ isVisible: async () => true });
        return locator({ count: async () => 0 });
      },
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "kanban-group-header" ? list([header]) : list([])) });
    await expect(new WebNewDriver(page).kanbanHeaderCreateVisible("Todo")).resolves.toBe(true);
  });

  it("falls back to the name text without a title marker", async () => {
    const header = locator({
      getByTestId: (id: string) => {
        if (id === "kanban-group-name") {
          return locator({
            getAttribute: async () => null,
            innerText: async () => "  Todo\n",
          });
        }
        if (id === "kanban-header-create") return locator({ isVisible: async () => true });
        return locator({ count: async () => 0 });
      },
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "kanban-group-header" ? list([header]) : list([])) });
    await expect(new WebNewDriver(page).kanbanHeaderCreateVisible("Todo")).resolves.toBe(true);
  });

  it("falls through unreadable headers to the flat column", async () => {
    const unreadable = locator({
      getByTestId: (id: string) =>
        id === "kanban-group-name"
          ? locator({
              getAttribute: async () => null,
              innerText: async () => {
                throw new Error("gone");
              },
            })
          : locator({ count: async () => 0 }),
    });
    const column = locator({
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
      getByTestId: (id: string) =>
        id === "kanban-header-create" ? locator({ isVisible: async () => true }) : locator({ count: async () => 0 }),
    });
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-group-header") return list([unreadable]);
        if (id === "kanban-column") return list([column]);
        return list([]);
      },
    });
    await expect(new WebNewDriver(page).kanbanHeaderCreateVisible("Todo")).resolves.toBe(true);
  });

  it("reads the create modal by its heading", async () => {
    const dialog = (visible: boolean) =>
      pageOf({
        getByRole: () =>
          locator({
            filter: () =>
              locator({
                first: () => locator({ count: async () => (visible ? 1 : 0), isVisible: async () => visible }),
              }),
          }),
      });
    await expect(new WebNewDriver(dialog(true)).kanbanCreateModalVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(dialog(false)).kanbanCreateModalVisible()).resolves.toBe(false);
  });
});

describe("kanban header menu", () => {
  function menuPage(entries: string[]): Page {
    return pageOf({
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? list(entries.map((entry) => locator({ innerText: async () => entry })))
          : locator({ count: async () => 0 }),
    });
  }

  it("reads the open menu entries without blanks", async () => {
    const page = menuPage([" Create work item ", "", "Add an existing work item"]);
    await expect(new WebNewDriver(page).kanbanHeaderMenuItems("Todo")).resolves.toEqual([
      "Create work item",
      "Add an existing work item",
    ]);
  });

  it("drops menu entries that will not read", async () => {
    const throwing = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-group-header")
          return list([headerWithCreate(locator({ click: async () => undefined }), "Todo")]);
        return list([]);
      },
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? list([
              locator({
                innerText: async () => {
                  throw new Error("gone");
                },
              }),
              locator({ innerText: async () => "Create work item" }),
            ])
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(throwing).kanbanHeaderMenuItems("Todo")).resolves.toEqual(["Create work item"]);
  });

  it("waits for a delayed menu mount before reading", async () => {
    // The menu portal paints a beat after the trigger click (ISS-036);
    // the read waits for the first entry instead of one-shotting zero.
    const waits: Array<number | undefined> = [];
    let counts = 0;
    const nodes = [
      locator({ innerText: async () => "Create work item" }),
      locator({ innerText: async () => "Add an existing work item" }),
    ];
    const page = pageOf({
      waitForTimeout: async (ms?: number) => void waits.push(ms),
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? locator({
              count: async () => {
                counts += 1;
                return counts <= 2 ? 0 : nodes.length;
              },
              nth: (index: number) => nodes[index]!,
            })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).kanbanHeaderMenuItems("Todo")).resolves.toEqual([
      "Create work item",
      "Add an existing work item",
    ]);
    expect(waits).toEqual([250, 250]);
  });

  it("reports no entries when the menu never opens", async () => {
    const page = menuPage([]);
    await expect(new WebNewDriver(page).kanbanHeaderMenuItems("Todo")).resolves.toEqual([]);
  });

  it("chooses the matching entry and throws when missing", async () => {
    const clicks: string[] = [];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? list([
              locator({ innerText: async () => "Create work item", click: async () => void clicks.push("create") }),
              locator({
                innerText: async () => "Add an existing work item",
                click: async () => void clicks.push("add"),
              }),
            ])
          : locator({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(page);
    await driver.kanbanHeaderMenuChoose("Todo", "Add an existing work item");
    expect(clicks).toEqual(["add"]);
    await expect(driver.kanbanHeaderMenuChoose("Todo", "Missing")).rejects.toThrow(/no header menu entry/);
  });
});

describe("kanban drags", () => {
  function dragPage(cards: Locator[], columns: Locator[] = []) {
    const { mouse, moves, events } = mouseStub();
    const page = pageOf({
      mouse,
      waitForTimeout: async () => undefined,
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list(cards);
        if (id === "kanban-column") return list(columns);
        return list([]);
      },
    });
    return { page, moves, events };
  }

  function dragColumn(name: string, rect: Box | null, cards: Locator[] = []): Locator {
    return locator({
      boundingBox: async () => rect,
      scrollIntoViewIfNeeded: async () => undefined,
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? name : null),
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list(cards);
        return locator({ count: async () => 0 });
      },
    });
  }

  it("drags a card before its target and settles on the new order", async () => {
    const source = dragCard("Third", { x: 0, y: 200, width: 300, height: 80 });
    const target = dragCard("First", { x: 0, y: 0, width: 300, height: 80 });
    // The board already shows the settled order, so the poll passes first try.
    const { page, moves, events } = dragPage([source, target]);
    await new WebNewDriver(page).kanbanDragCardBefore("Third", "First");
    expect(events).toEqual(["move", "down", "move", "up"]);
    expect(moves[0]).toEqual({ x: 150, y: 214, steps: undefined });
    expect(moves[1]).toEqual({ x: 150, y: 8, steps: 8 });
  });

  it("throws when either drag card has no box", async () => {
    const boxed = dragCard("Third", { x: 0, y: 0, width: 300, height: 80 });
    const flat = dragCard("First", null);
    await expect(new WebNewDriver(dragPage([flat, boxed]).page).kanbanDragCardBefore("Third", "First")).rejects.toThrow(
      /no box/
    );
    await expect(new WebNewDriver(dragPage([boxed, flat]).page).kanbanDragCardBefore("Third", "First")).rejects.toThrow(
      /no box/
    );
  });

  it("attempts a drop without settling", async () => {
    const waits: Array<number | undefined> = [];
    const source = dragCard("Beta", { x: 0, y: 0, width: 300, height: 80 });
    const target = dragCard("Gamma", { x: 0, y: 100, width: 300, height: 80 });
    const { mouse, events } = mouseStub();
    const page = pageOf({
      mouse,
      waitForTimeout: async (ms?: number) => void waits.push(ms),
      getByTestId: (id: string) => (id === "kanban-card" ? list([source, target]) : list([])),
    });
    await new WebNewDriver(page).kanbanAttemptCardBefore("Beta", "Gamma");
    expect(events).toEqual(["move", "down", "move", "up"]);
    expect(waits).toContain(3_000);
  });

  it("throws an attempt without boxes", async () => {
    const boxed = dragCard("Beta", { x: 0, y: 0, width: 300, height: 80 });
    const flat = dragCard("Gamma", null);
    const attempt = (cards: Locator[]) =>
      new WebNewDriver(dragPage(cards).page).kanbanAttemptCardBefore("Beta", "Gamma");
    await expect(attempt([flat, boxed])).rejects.toThrow(/no box/);
    await expect(attempt([boxed, flat])).rejects.toThrow(/no box/);
  });

  it("drags a card to the column end and settles on its arrival", async () => {
    const scrolled: string[] = [];
    const source = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    const column = locator({
      boundingBox: async () => ({ x: 350, y: 0, width: 350, height: 600 }),
      scrollIntoViewIfNeeded: async () => void scrolled.push("done"),
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Done" : null),
      getByTestId: (id: string) => (id === "kanban-card" ? list([dragCard("Fix login", null)]) : list([])),
    });
    const { page, moves } = dragPage([source], [column]);
    await new WebNewDriver(page).kanbanDragCardToColumnEnd("Fix login", "Done");
    expect(scrolled).toEqual(["done"]);
    expect(moves[0]).toEqual({ x: 150, y: 14, steps: undefined });
    expect(moves[1]).toEqual({ x: 525, y: 588, steps: 8 });
  });

  it("presses the name row instead of the card center", async () => {
    // The geometric center lands on chip buttons once the card is dressed
    // (labels, due date), and the card ignores presses starting on
    // controls — so the press goes to the draggable name row (ISS-038).
    const source = dragCard(
      "Fix login",
      { x: 0, y: 0, width: 300, height: 102 },
      { x: 12, y: 12, width: 200, height: 20 }
    );
    const column = dragColumn("Done", { x: 350, y: 0, width: 350, height: 600 }, [dragCard("Fix login", null)]);
    const { page, moves } = dragPage([source], [column]);
    await new WebNewDriver(page).kanbanDragCardToColumnEnd("Fix login", "Done");
    expect(moves[0]).toEqual({ x: 112, y: 22, steps: undefined });
  });

  it("presses near the top when the name row has no box", async () => {
    const source = dragCard("Fix login", { x: 0, y: 50, width: 300, height: 102 });
    const column = dragColumn("Done", { x: 350, y: 0, width: 350, height: 600 }, [dragCard("Fix login", null)]);
    const { page, moves } = dragPage([source], [column]);
    await new WebNewDriver(page).kanbanDragCardToColumnEnd("Fix login", "Done");
    expect(moves[0]).toEqual({ x: 150, y: 64, steps: undefined });
  });

  it("throws a column-end drag without boxes", async () => {
    const boxed = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    const flat = dragCard("Fix login", null);
    const column = (rect: Box | null) => dragColumn("Done", rect);
    const attempt = (card: Locator, target: Locator) =>
      new WebNewDriver(dragPage([card], [target]).page).kanbanDragCardToColumnEnd("Fix login", "Done");
    await expect(attempt(flat, column({ x: 0, y: 0, width: 10, height: 10 }))).rejects.toThrow(/no box/);
    await expect(attempt(boxed, column(null))).rejects.toThrow(/no box/);
  });

  it("holds near each viewport edge along the card's own axis", async () => {
    const waits: Array<number | undefined> = [];
    const source = dragCard("Last", { x: 500, y: 300, width: 300, height: 80 });
    const { mouse, moves } = mouseStub();
    const page = pageOf({
      mouse,
      waitForTimeout: async (ms?: number) => void waits.push(ms),
      viewportSize: () => ({ width: 1280, height: 720 }),
      getByTestId: (id: string) => (id === "kanban-card" ? list([source]) : list([])),
    });
    const driver = new WebNewDriver(page);
    await driver.kanbanDragHoldNearEdge("Last", "right", 500);
    await driver.kanbanDragHoldNearEdge("Last", "left", 500);
    await driver.kanbanDragHoldNearEdge("Last", "top", 500);
    await driver.kanbanDragHoldNearEdge("Last", "bottom", 500);
    expect(waits).toContain(500);
    expect(moves.map((move) => [move.x, move.y])).toEqual([
      [650, 314],
      [1268, 314],
      [650, 314],
      [12, 314],
      [650, 314],
      [650, 12],
      [650, 314],
      [650, 708],
    ]);
  });

  it("rides out window churn when grabbing the held card", async () => {
    // The virtualized window flickers names while it settles after a
    // scroll or page append; the grab retries instead of throwing on the
    // first unreadable sample (ISS-043).
    let reads = 0;
    const source = locator({
      boundingBox: async () => ({ x: 500, y: 300, width: 300, height: 80 }),
      getByTestId: (id: string) =>
        id === "kanban-card-name"
          ? locator({
              innerText: async () => {
                reads += 1;
                return reads <= 2 ? "" : "Last";
              },
            })
          : locator({ count: async () => 0 }),
    });
    const waits: Array<number | undefined> = [];
    const { mouse, moves } = mouseStub();
    const page = pageOf({
      mouse,
      waitForTimeout: async (ms?: number) => void waits.push(ms),
      viewportSize: () => ({ width: 1280, height: 720 }),
      getByTestId: (id: string) => (id === "kanban-card" ? list([source]) : list([])),
    });
    await new WebNewDriver(page).kanbanDragHoldNearEdge("Last", "right", 10);
    expect(reads).toBe(3);
    expect(waits).toEqual(expect.arrayContaining([1000, 1000]));
    expect(moves[0]).toEqual({ x: 650, y: 314, steps: undefined });
  });

  it("holds with the fallback viewport and throws without a box", async () => {
    const source = dragCard("Last", { x: 0, y: 0, width: 300, height: 80 });
    const { mouse, moves } = mouseStub();
    const page = pageOf({
      mouse,
      waitForTimeout: async () => undefined,
      viewportSize: () => null,
      getByTestId: (id: string) => (id === "kanban-card" ? list([source]) : list([])),
    });
    await new WebNewDriver(page).kanbanDragHoldNearEdge("Last", "right", 10);
    expect(moves[1]).toEqual({ x: 1268, y: 14, steps: 10 });
    const flat = dragCard("Last", null);
    const missing = pageOf({
      mouse: mouseStub().mouse,
      getByTestId: (id: string) => (id === "kanban-card" ? list([flat]) : list([])),
    });
    await expect(new WebNewDriver(missing).kanbanDragHoldNearEdge("Last", "right", 10)).rejects.toThrow(/no box/);
  });
});

describe("kanban delete flow", () => {
  function deletePage(modalVisible: boolean) {
    const { mouse } = mouseStub();
    const source = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    return pageOf({
      mouse,
      waitForTimeout: async () => undefined,
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list([source]);
        if (id === "kanban-delete-zone") {
          return locator({ boundingBox: async () => ({ x: 300, y: 0, width: 200, height: 40 }) });
        }
        return list([]);
      },
      getByRole: () =>
        locator({
          filter: () =>
            locator({
              count: async () => (modalVisible ? 1 : 0),
              first: () => locator({ count: async () => (modalVisible ? 1 : 0), isVisible: async () => modalVisible }),
              getByRole: () => locator({ first: () => locator({ click: async () => undefined }) }),
            }),
        }),
    });
  }

  it("drags a card onto the zone and settles on the modal", async () => {
    await new WebNewDriver(deletePage(true)).kanbanDragCardToDelete("Fix login");
    await expect(new WebNewDriver(deletePage(true)).kanbanDeleteModalVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(deletePage(false)).kanbanDeleteModalVisible()).resolves.toBe(false);
  });

  it("throws a delete drag without boxes", async () => {
    const flat = dragCard("Fix login", null);
    const page = pageOf({
      mouse: mouseStub().mouse,
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list([flat]);
        if (id === "kanban-delete-zone") return locator({ boundingBox: async () => null });
        return list([]);
      },
    });
    await expect(new WebNewDriver(page).kanbanDragCardToDelete("Fix login")).rejects.toThrow(/no box/);
    const boxed = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    const zoneLess = pageOf({
      mouse: mouseStub().mouse,
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list([boxed]);
        if (id === "kanban-delete-zone") return locator({ boundingBox: async () => null });
        return list([]);
      },
    });
    await expect(new WebNewDriver(zoneLess).kanbanDragCardToDelete("Fix login")).rejects.toThrow(/no box/);
  });

  it("confirms the delete and settles on the closed modal", async () => {
    let open = true;
    const clicks: string[] = [];
    const modal = locator({
      count: async () => (open ? 1 : 0),
      isVisible: async () => open,
      getByRole: () =>
        locator({
          first: () =>
            locator({
              click: async () => {
                open = false;
                clicks.push("delete");
              },
            }),
        }),
    });
    const page = pageOf({
      getByRole: () => locator({ filter: () => locator({ first: () => modal }) }),
    });
    await new WebNewDriver(page).kanbanConfirmDelete();
    expect(clicks).toEqual(["delete"]);
  });

  it("confirms from the page scope without a modal anchor", async () => {
    const clicks: string[] = [];
    const page = pageOf({
      getByRole: (role: string) => {
        if (role !== "button") {
          return locator({
            filter: () => locator({ count: async () => 0, first: () => locator({ count: async () => 0 }) }),
          });
        }
        return locator({ first: () => locator({ click: async () => void clicks.push("delete") }) });
      },
    });
    await new WebNewDriver(page).kanbanConfirmDelete();
    expect(clicks).toEqual(["delete"]);
  });
});

describe("kanban hold overlay and toasts", () => {
  function holdPage(overlay: string | null) {
    const { mouse, events } = mouseStub();
    const source = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    const column = locator({
      boundingBox: async () => ({ x: 350, y: 0, width: 350, height: 600 }),
      scrollIntoViewIfNeeded: async () => undefined,
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Done" : null),
      getByTestId: (id: string) =>
        id === "kanban-drop-overlay" && overlay !== null
          ? locator({ innerText: async () => overlay })
          : locator({ count: async () => 0 }),
    });
    const page = pageOf({
      mouse,
      waitForTimeout: async () => undefined,
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list([source]);
        if (id === "kanban-column") return list([column]);
        return list([]);
      },
    });
    return { page, events };
  }

  it("holds over a column and reads its overlay", async () => {
    const { page, events } = holdPage("  Cannot move to a completed cycle\n");
    await expect(new WebNewDriver(page).kanbanDragHoldOverColumn("Fix login", "Done")).resolves.toEqual({
      overlay: "Cannot move to a completed cycle",
    });
    expect(events).toEqual(["move", "down", "move", "up"]);
  });

  it("reads no overlay over an accepting column", async () => {
    const { page } = holdPage(null);
    await expect(new WebNewDriver(page).kanbanDragHoldOverColumn("Fix login", "Done")).resolves.toEqual({
      overlay: null,
    });
  });

  it("throws an overlay hold without boxes", async () => {
    const boxed = dragCard("Fix login", { x: 0, y: 0, width: 300, height: 80 });
    const flat = dragCard("Fix login", null);
    const column = (rect: Box | null) =>
      locator({
        boundingBox: async () => rect,
        scrollIntoViewIfNeeded: async () => undefined,
        getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Done" : null),
      });
    const attempt = (card: Locator, target: Locator) => {
      const { mouse } = mouseStub();
      const page = pageOf({
        mouse,
        getByTestId: (id: string) => {
          if (id === "kanban-card") return list([card]);
          if (id === "kanban-column") return list([target]);
          return list([]);
        },
      });
      return new WebNewDriver(page).kanbanDragHoldOverColumn("Fix login", "Done");
    };
    await expect(attempt(flat, column({ x: 0, y: 0, width: 10, height: 10 }))).rejects.toThrow(/no box/);
    await expect(attempt(boxed, column(null))).rejects.toThrow(/no box/);
  });

  function toastPage(entries: Array<{ title: string; message: string }>): Page {
    return pageOf({
      locator: (selector: string) =>
        selector === 'xpath=//*[@aria-label="Notifications"]//*[@role="dialog" or @role="alertdialog"]'
          ? list(
              entries.map((entry) =>
                locator({
                  innerText: async () => `${entry.title}\n${entry.message}`,
                })
              )
            )
          : locator({ count: async () => 0 }),
    });
  }

  it("reads the newest toast and ignores an empty stack", async () => {
    const page = toastPage([{ title: "Cannot move the work item", message: "Cards stay put." }]);
    await expect(new WebNewDriver(page).boardLastToast()).resolves.toEqual({
      title: "Cannot move the work item",
      message: "Cards stay put.",
    });
    await expect(new WebNewDriver(toastPage([])).boardLastToast()).resolves.toBeNull();
    await expect(new WebNewDriver(toastPage([{ title: " ", message: " " }])).boardLastToast()).resolves.toBeNull();
  });

  it("treats an unreadable toast as empty", async () => {
    const unreadable = locator({
      innerText: async () => {
        throw new Error("gone");
      },
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'xpath=//*[@aria-label="Notifications"]//*[@role="dialog" or @role="alertdialog"]'
          ? list([unreadable])
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).boardLastToast()).resolves.toBeNull();
  });
});

describe("kanban column paging and scroll", () => {
  it("scrolls a column to its end and reports its scroll", async () => {
    const node = { scrollLeft: 0, scrollTop: 40, scrollHeight: 600 };
    const column = locator({
      getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
      getByTestId: (id: string) =>
        id === "kanban-column-body"
          ? locator({
              evaluate: async (fn: (target: typeof node) => void) => void fn(node),
            })
          : locator({ count: async () => 0 }),
    });
    const waits: Array<number | undefined> = [];
    const page = pageOf({
      getByTestId: (id: string) => (id === "kanban-column" ? list([column]) : list([])),
      waitForTimeout: async (ms?: number) => void waits.push(ms),
    });
    const driver = new WebNewDriver(page);
    await driver.kanbanColumnScrollEnd("Todo");
    expect(node.scrollTop).toBe(600);
    expect(waits).toEqual([2_000]);
    await driver.kanbanColumnLoadMore("Todo");
    expect(node.scrollTop).toBe(600);
  });

  it("reports flat columns without an explicit entry", async () => {
    await expect(new WebNewDriver(pageOf()).kanbanColumnHasLoadMore("Todo")).resolves.toBe(false);
  });

  it("reports a column's loading marker", async () => {
    const column = (loading: boolean) =>
      locator({
        getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
        getByTestId: (id: string) =>
          id === "kanban-column-loading"
            ? locator({ count: async () => (loading ? 1 : 0), isVisible: async () => loading })
            : locator({ count: async () => 0 }),
      });
    const page = (loading: boolean) =>
      pageOf({ getByTestId: (id: string) => (id === "kanban-column" ? list([column(loading)]) : list([])) });
    await expect(new WebNewDriver(page(true)).kanbanColumnLoading("Todo")).resolves.toBe(true);
    await expect(new WebNewDriver(page(false)).kanbanColumnLoading("Todo")).resolves.toBe(false);
  });

  it("reads board and column scroll offsets", async () => {
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-board") return locator({ evaluate: async () => ({ x: 120, y: 0 }) });
        if (id === "kanban-column") {
          return list([
            locator({
              getAttribute: async (attribute: string) => (attribute === "data-column-name" ? "Todo" : null),
              getByTestId: (child: string) =>
                child === "kanban-column-body" ? locator({ evaluate: async () => ({ x: 0, y: 45 }) }) : locator(),
            }),
          ]);
        }
        return list([]);
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanBoardScroll()).resolves.toEqual({ x: 120, y: 0 });
    await expect(driver.kanbanColumnScroll("Todo")).resolves.toEqual({ x: 0, y: 45 });
  });
});

describe("kanban cells", () => {
  function cellPage(
    options: { loadMore?: boolean; names?: Array<string | null>; key?: string | null; missingCell?: boolean } = {}
  ) {
    const names = options.names ?? ["Fix login"];
    const key = options.key === undefined ? "todo" : options.key;
    const cell = locator({
      getByTestId: (id: string) => {
        if (id === "kanban-card") {
          return list(names.map((name) => (name === null ? placeholderCard() : namedCard(name))));
        }
        if (id === "kanban-load-more") {
          const shown = options.loadMore === true;
          return locator({ count: async () => (shown ? 1 : 0), isVisible: async () => shown });
        }
        return locator({ count: async () => 0 });
      },
    });
    const lane = locator({
      getByTestId: (id: string) => {
        if (id === "kanban-lane-name") return locator({ getAttribute: async () => "Bug" });
        return locator({ count: async () => 0 });
      },
      locator: (selector: string) =>
        !options.missingCell && selector === `[data-testid="kanban-cell"][data-column-key="${key}"]`
          ? cell
          : locator({ count: async () => 0 }),
    });
    return pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-lane") return list([lane]);
        if (id === "kanban-group-header") return list([groupHeaderStub("Todo", key)]);
        return list([]);
      },
    });
  }

  it("reads a cell's cards with placeholders as blanks", async () => {
    const page = cellPage({ names: ["Fix login", null] });
    await expect(new WebNewDriver(page).kanbanCellCards("Todo", "Bug")).resolves.toEqual(["Fix login", ""]);
  });

  it("reports and activates a cell's load-more entry", async () => {
    const clicks: string[] = [];
    const names: string[] = [];
    const cell = locator({
      getByTestId: (id: string) => {
        if (id === "kanban-card") return list(names.map((name) => namedCard(name)));
        if (id === "kanban-load-more") {
          return locator({
            isVisible: async () => true,
            click: async () => {
              clicks.push("more");
              names.push("Fix login");
            },
          });
        }
        return locator({ count: async () => 0 });
      },
    });
    const lane = locator({
      getByTestId: (id: string) =>
        id === "kanban-lane-name" ? locator({ getAttribute: async () => "Bug" }) : locator({ count: async () => 0 }),
      locator: () => cell,
    });
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-lane") return list([lane]);
        if (id === "kanban-group-header") return list([groupHeaderStub("Todo", "todo")]);
        return list([]);
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanCellHasLoadMore("Todo", "Bug")).resolves.toBe(true);
    await driver.kanbanCellLoadMore("Todo", "Bug");
    expect(clicks).toEqual(["more"]);
    await expect(new WebNewDriver(cellPage({})).kanbanCellHasLoadMore("Todo", "Bug")).resolves.toBe(false);
  });

  it("throws for unknown lanes, columns and missing cells", async () => {
    const page = cellPage({});
    const driver = new WebNewDriver(page);
    await expect(driver.kanbanCellCards("Todo", "Nope")).rejects.toThrow(/no kanban swimlane/);
    await expect(driver.kanbanCellCards("Nope", "Bug")).rejects.toThrow(/no kanban cell/);
    const keyless = cellPage({ key: null });
    await expect(new WebNewDriver(keyless).kanbanCellCards("Todo", "Bug")).rejects.toThrow(/no kanban cell/);
    const missing = cellPage({ missingCell: true });
    await expect(new WebNewDriver(missing).kanbanCellCards("Todo", "Bug")).rejects.toThrow(/no kanban cell/);
  });

  it("skips headers whose name will not read", async () => {
    const lane = locator({
      getByTestId: (id: string) =>
        id === "kanban-lane-name" ? locator({ getAttribute: async () => "Bug" }) : locator({ count: async () => 0 }),
      locator: () => locator({ getByTestId: () => list([]) }),
    });
    const unreadable = locator({
      getByTestId: (id: string) =>
        id === "kanban-group-name"
          ? locator({
              getAttribute: async () => null,
              innerText: async () => {
                throw new Error("gone");
              },
            })
          : locator({ count: async () => 0 }),
    });
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "kanban-lane") return list([lane]);
        if (id === "kanban-group-header") return list([unreadable, groupHeaderStub("Todo", "todo")]);
        return list([]);
      },
    });
    await expect(new WebNewDriver(page).kanbanCellCards("Todo", "Bug")).resolves.toEqual([]);
  });
});
