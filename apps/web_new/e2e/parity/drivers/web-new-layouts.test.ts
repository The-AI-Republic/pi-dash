// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver empty-state and overlay methods
// (NEWFRONT-637, ISS-068–074). A stub Page records each selector the
// method reads, so a markup change that drops a parity hook fails here
// before the parity stack is brought up.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  isChecked?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  innerText?: () => Promise<string>;
  nth?: (index: number) => StubLocator;
  first?: () => StubLocator;
  last?: () => StubLocator;
  locator?: (selector: string, options?: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string | RegExp, options?: unknown) => StubLocator;
  getByPlaceholder?: (text: string, options?: unknown) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  evaluate?: (fn: (node: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isDisabled: async () => false,
    isChecked: async () => true,
    click: async () => undefined,
    fill: async () => undefined,
    waitFor: async () => undefined,
    innerText: async () => "",
    evaluate: async () => null,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    locator: () => node,
    getByRole: () => node,
    getByText: () => node,
    getByPlaceholder: () => node,
    filter: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

interface PageStub {
  page: Page;
  selectors: string[];
  roles: Array<{ role: string; name: unknown }>;
  clicks: string[];
  keys: string[];
  filled: string[];
  waits: number;
  responses: Array<{ predicate: (response: never) => unknown; options: unknown }>;
}

function pageOf(
  parts: {
    locator?: (selector: string) => Locator;
    role?: (role: string, options: Record<string, unknown> | undefined) => Locator;
    placeholder?: (text: string) => Locator;
    testId?: (id: string) => Locator;
    url?: string;
  } = {}
): PageStub {
  const state: PageStub = {
    selectors: [],
    roles: [],
    clicks: [],
    keys: [],
    filled: [],
    waits: 0,
    responses: [],
    page: undefined as unknown as Page,
  };
  state.page = {
    keyboard: {
      press: async (key: string) => {
        state.keys.push(key);
      },
    },
    getByRole: (role: string, options?: Record<string, unknown>) => {
      state.roles.push({ role, name: options?.["name"] });
      return parts.role?.(role, options) ?? locator();
    },
    getByPlaceholder: (text: string) => parts.placeholder?.(text) ?? locator(),
    getByTestId: (id: string) => parts.testId?.(id) ?? locator(),
    waitForResponse: async (predicate: (response: never) => unknown, options: unknown) => {
      state.responses.push({ predicate, options });
      return {};
    },
    locator: (selector: string) => {
      state.selectors.push(selector);
      return parts.locator?.(selector) ?? locator();
    },
    waitForTimeout: async () => {
      state.waits += 1;
    },
    url: () => parts.url ?? "https://p.test/acme/projects/p/issues",
    evaluate: async (fn: (arg?: unknown) => unknown, arg?: unknown) => fn(arg),
    reload: async () => null,
  } as unknown as Page;
  return state;
}

describe("web_new empty-state driver", () => {
  it("reads the visible card's heading", async () => {
    const { page, selectors } = pageOf({
      locator: (selector) => {
        if (selector === '[data-testid="issues-empty-state"]:visible') {
          return locator({ getByRole: () => locator({ innerText: async () => "  No project  " }) });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).layoutsEmptyTitle()).resolves.toBe("No project");
    expect(selectors).toContain('[data-testid="issues-empty-state"]:visible');
  });

  it("reads the title span when the heading also carries the description", async () => {
    const { page } = pageOf({
      locator: (selector) => {
        if (selector === '[data-testid="issues-empty-state"]:visible') {
          return locator({
            getByRole: () =>
              locator({
                innerText: async () => "Start with your first work item.\nWork items are the building blocks.",
                locator: () => locator({ innerText: async () => "Start with your first work item." }),
              }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).layoutsEmptyTitle()).resolves.toBe("Start with your first work item.");
  });

  it("falls back to the heading text when no title span exists", async () => {
    const { page } = pageOf({
      locator: (selector) => {
        if (selector === '[data-testid="issues-empty-state"]:visible') {
          return locator({
            getByRole: () =>
              locator({
                innerText: async () => "No matching results.",
                locator: () => locator({ count: async () => 0 }),
              }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).layoutsEmptyTitle()).resolves.toBe("No matching results.");
  });

  it("reads nothing when no card is mounted", async () => {
    const { page } = pageOf({
      locator: (selector) =>
        selector === '[data-testid="issues-empty-state"]:visible' ? locator({ count: async () => 0 }) : locator(),
    });
    await expect(new WebNewDriver(page).layoutsEmptyTitle()).resolves.toBeNull();
    await expect(new WebNewDriver(page).layoutsEmptyActions()).resolves.toEqual([]);
  });

  it("lists every action with its enabled state", async () => {
    const actions = locator({
      count: async () => 2,
      nth: (index) =>
        locator({
          innerText: async () => (index === 0 ? " Create work item " : "Add existing work item"),
          isDisabled: async () => index === 1,
        }),
    });
    const { page } = pageOf({
      locator: (selector) =>
        selector === '[data-testid="issues-empty-state"]:visible' ? locator({ locator: () => actions }) : locator(),
    });
    await expect(new WebNewDriver(page).layoutsEmptyActions()).resolves.toEqual([
      { label: "Create work item", disabled: false },
      { label: "Add existing work item", disabled: true },
    ]);
  });

  it("clicks an action by its label inside the visible card", async () => {
    const clicks: string[] = [];
    const { page } = pageOf({
      locator: (selector) =>
        selector === '[data-testid="issues-empty-state"]:visible'
          ? locator({
              locator: (inner) => {
                expect(inner).toBe("button");
                return locator({
                  first: () => locator({ click: async () => void clicks.push("button") }),
                });
              },
            })
          : locator(),
    });
    await new WebNewDriver(page).layoutsEmptyChoose("Add existing work item");
    expect(clicks).toEqual(["button"]);
  });

  it("reads the profile activity heading", async () => {
    const { page, roles } = pageOf({
      role: (role, options) =>
        role === "heading" && options?.["name"] === "Recent activity"
          ? locator({ count: async () => 1 })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsProfileActivityVisible()).resolves.toBe(true);
    expect(roles).toContainEqual({ role: "heading", name: "Recent activity" });
  });

  it("reports the current url", async () => {
    const { page } = pageOf({ url: "https://p.test/acme/projects/p/automations" });
    await expect(new WebNewDriver(page).layoutsCurrentUrl()).resolves.toBe(
      "https://p.test/acme/projects/p/automations"
    );
  });
});

describe("web_new work-item modal driver", () => {
  it("finds the dialog through its Title field and reads the heading", async () => {
    const { page, roles } = pageOf({
      role: (role) =>
        role === "dialog"
          ? locator({ getByRole: () => locator({ innerText: async () => "Create new work item" }) })
          : locator(),
      placeholder: (text) => (text === "Title" ? locator() : locator({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(page).layoutsWorkItemModalVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(page).layoutsWorkItemModalTitle()).resolves.toBe("Create new work item");
    expect(roles.filter((row) => row.role === "dialog").length).toBeGreaterThan(0);
  });

  it("reads the edit-mode heading the scenario matches by intent (ISS-060)", async () => {
    // The same dialog serves edit mode; its heading wording is the app's
    // own copy (spec §7, NEWFRONT-934). The driver must surface whatever
    // sentence the heading carries, trimmed, so the scenario can pin the
    // intent with a case-insensitive match instead of the old app's text.
    const filters: unknown[] = [];
    const dialog = locator({ count: async () => 1 });
    dialog.getByRole = () => locator({ first: () => locator({ innerText: async () => "  Update work item  " }) });
    dialog.filter = (options: unknown) => {
      filters.push(options);
      return dialog;
    };
    const { page } = pageOf({
      role: (role) => (role === "dialog" ? dialog : locator()),
      placeholder: (text) => (text === "Title" ? locator() : locator({ count: async () => 0 })),
    });

    const heading = await new WebNewDriver(page).layoutsWorkItemModalTitle();
    expect(heading).toBe("Update work item");
    expect(heading ?? "").toMatch(/update/i);
    // The dialog is still picked out by its Title field, so a strip or
    // confirmation dialog with another heading never answers for it.
    expect(filters).toHaveLength(1);
    expect(Object.keys(filters[0] as Record<string, unknown>)).toEqual(["has"]);
    expect(await new WebNewDriver(page).layoutsWorkItemModalVisible()).toBe(true);
  });

  it("reports no modal when no dialog exists", async () => {
    const { page } = pageOf({
      role: () => locator({ count: async () => 0 }),
      placeholder: () => locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsWorkItemModalVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(page).layoutsWorkItemModalTitle()).resolves.toBeNull();
  });

  it("fills the title field", async () => {
    const filled: string[] = [];
    const field = locator({
      fill: async (value: string) => {
        filled.push(value);
      },
    });
    const { page } = pageOf({
      role: (role) =>
        role === "dialog"
          ? locator({ filter: () => locator({ getByPlaceholder: () => locator({ first: () => field }) }) })
          : locator(),
    });
    await new WebNewDriver(page).layoutsWorkItemModalSetTitle("Parity item");
    expect(filled).toEqual(["Parity item"]);
  });

  it("closes through Escape once the dialog is gone", async () => {
    let open = true;
    const { page, keys } = pageOf({
      role: (role) => (role === "dialog" ? locator({ count: async () => (open ? 1 : 0) }) : locator()),
      placeholder: () => locator(),
    });
    (page as unknown as { keyboard: { press: (key: string) => Promise<void> } }).keyboard.press = async (
      key: string
    ) => {
      keys.push(key);
      open = false;
    };
    await new WebNewDriver(page).layoutsWorkItemModalClose();
    expect(keys).toEqual(["Escape"]);
  });

  it("submits a create once and waits for the dialog to close", async () => {
    let open = true;
    const clicks: string[] = [];
    const save = locator({
      count: async () => 1,
      first: () =>
        locator({
          click: async () => {
            clicks.push("Save");
            open = false;
          },
        }),
    });
    const dialog = locator({
      count: async () => (open ? 1 : 0),
      getByRole: (role: string) => (role === "button" ? save : locator({ count: async () => 0 })),
    });
    const { page } = pageOf({
      role: (role) => (role === "dialog" ? dialog : locator()),
      placeholder: () => locator(),
    });
    await new WebNewDriver(page).layoutsWorkItemModalSubmit();
    expect(clicks).toEqual(["Save"]);
  });

  it("reports whether the dialog carries a text", async () => {
    const { page } = pageOf({
      role: (role, options) =>
        role === "dialog"
          ? locator({
              filter: () => locator(),
              getByText: () => locator({ count: async () => (options === undefined ? 1 : 0) }),
              first: () => locator({ evaluate: async () => true }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).layoutsWorkItemModalHasText("Parity item")).resolves.toBe(true);
  });
});

describe("web_new add-existing modal driver", () => {
  it("reads the dialog marker from its submit control", async () => {
    const { page, roles } = pageOf({
      role: (role, options) =>
        role === "button" && options?.["name"] === "Add selected work items"
          ? locator({ count: async () => 1 })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsAddExistingModalVisible()).resolves.toBe(true);
    expect(roles.some((row) => row.role === "button")).toBe(true);
  });

  it("lists the option names once the search returns rows", async () => {
    const { page, selectors } = pageOf({
      locator: (selector) => {
        if (selector === 'label[for^="issue-"]') {
          return locator({
            count: async () => 2,
            nth: (index) =>
              locator({
                locator: () =>
                  locator({ last: () => locator({ innerText: async () => (index === 0 ? "First" : " Second ") }) }),
              }),
          });
        }
        if (selector === "span") return locator({ innerText: async () => "row" });
        return locator();
      },
    });
    const names = await new WebNewDriver(page).layoutsAddExistingModalIssueNames();
    expect(names.length).toBe(2);
    expect(selectors).toContain('label[for^="issue-"]');
  });

  it("returns no names when the search finds nothing", async () => {
    const { page } = pageOf({
      locator: (selector) => (selector === 'label[for^="issue-"]' ? locator({ count: async () => 0 }) : locator()),
      role: (role) => (role === "button" ? locator({ count: async () => 0 }) : locator()),
    });
    const getByText = (page as unknown as { getByText: (text: string) => Locator }).getByText;
    (page as unknown as { getByText: (text: string) => Locator }).getByText = (text: string) =>
      text === "No work items found" ? locator({ count: async () => 1 }) : getByText(text);
    await expect(new WebNewDriver(page).layoutsAddExistingModalIssueNames()).resolves.toEqual([]);
  });
});

describe("web_new visible issue names", () => {
  it("reads every non-blank content paragraph in order (NEWFRONT-662)", async () => {
    const { page, roles } = pageOf({
      role: (role) =>
        role === "main"
          ? locator({
              locator: (selector: string) =>
                selector === "p"
                  ? locator({ allTextContents: async () => ["  Cloud first issue  ", "", "Cloud second issue"] })
                  : locator(),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).visibleIssueNames()).resolves.toEqual([
      "Cloud first issue",
      "Cloud second issue",
    ]);
    expect(roles).toContainEqual({ role: "main", name: undefined });
  });
});

describe("web_new calendar options picker", () => {
  function optionsPage(optionVisible: boolean, clicks: string[]): Page {
    const option = locator({
      isVisible: async () => optionVisible,
      click: async () => {
        clicks.push("option");
      },
    });
    const optionsButton = locator({
      click: async () => {
        clicks.push("options");
      },
    });
    const root = locator({ getByRole: () => optionsButton });
    return {
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] !== "Options" ? option : locator(),
      getByTestId: () => root,
      waitForTimeout: async () => undefined,
    } as unknown as Page;
  }

  it("clicks an already-visible option without toggling Options shut", async () => {
    const clicks: string[] = [];
    await new WebNewDriver(optionsPage(true, clicks)).layoutsCalSetMode("week");
    expect(clicks).toEqual(["option"]);
  });

  it("opens the panel first when the option is not visible", async () => {
    const clicks: string[] = [];
    await new WebNewDriver(optionsPage(false, clicks)).layoutsCalSetMode("week");
    expect(clicks).toEqual(["options", "option"]);
  });
});

describe("web_new calendar month picker", () => {
  function pickerPage(initiallyOpen: boolean, clicks: string[]): Page {
    let open = initiallyOpen;
    const months = locator({
      count: async () => 2,
      nth: (index: number) => locator({ innerText: async () => ["Jan", "Feb"][index] ?? "" }),
    });
    const panel = locator({
      isVisible: async () => open,
      getByTestId: () => months,
    });
    const title = locator({
      click: async () => {
        clicks.push("title");
        open = !open;
      },
    });
    return {
      getByTestId: (id: string) => (id === "calendar-month-picker" ? panel : title),
      waitForTimeout: async () => undefined,
    } as unknown as Page;
  }

  it("opens a closed picker and toggles it shut on close", async () => {
    const clicks: string[] = [];
    const driver = new WebNewDriver(pickerPage(false, clicks));
    await expect(driver.layoutsCalMonthPickerMonths()).resolves.toEqual(["Jan", "Feb"]);
    expect(clicks).toEqual(["title", "title"]);
  });

  it("reuses an open picker and only clicks to close it", async () => {
    const clicks: string[] = [];
    const driver = new WebNewDriver(pickerPage(true, clicks));
    await expect(driver.layoutsCalMonthPickerMonths()).resolves.toEqual(["Jan", "Feb"]);
    expect(clicks).toEqual(["title"]);
  });
});

describe("web_new hover card reads", () => {
  function hoverPage(
    opts: { region: number; wide: number; blocks: number; cardText: string | null },
    seen: string[]
  ): PageStub {
    const stub = pageOf({
      role: () =>
        locator({
          locator: () => locator({ count: async () => opts.region, hover: async () => void seen.push("row") }),
        }),
      locator: (selector: string) => {
        if (selector === 'a[data-row-anchor="true"]') {
          return locator({ count: async () => opts.wide, hover: async () => void seen.push("wide") });
        }
        if (selector === 'a[id^="issue-"]') {
          return locator({ count: async () => opts.blocks, hover: async () => void seen.push("block") });
        }
        return locator();
      },
    });
    const card =
      opts.cardText === null
        ? locator({ count: async () => 0 })
        : locator({
            evaluate: async () => ({ text: opts.cardText, priorityIcon: "circle-alert", dateColor: "red" }),
          });
    (stub.page as unknown as Record<string, unknown>).getByTestId = () => card;
    return stub;
  }

  it("hovers the row anchor, reads the open card, and dismisses it", async () => {
    const seen: string[] = [];
    const { page, keys } = hoverPage({ region: 1, wide: 0, blocks: 0, cardText: "PAR-1 Range issue" }, seen);
    const card = await new WebNewDriver(page).hoverCardRead("Range issue");
    expect(card.text).toBe("PAR-1 Range issue");
    expect(seen).toEqual(["row"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("falls back to calendar blocks when no row anchor matches", async () => {
    const seen: string[] = [];
    const { page } = hoverPage({ region: 0, wide: 0, blocks: 1, cardText: "PAR-1 Range issue" }, seen);
    const card = await new WebNewDriver(page).hoverCardRead("Range issue");
    expect(card.text).toBe("PAR-1 Range issue");
    expect(seen).toEqual(["block"]);
  });

  it("waits for a delayed card mount before reading", async () => {
    // The calendar hover mounts its card after a short delay (ISS-133);
    // the read waits for the mount instead of one-shotting the count.
    const seen: string[] = [];
    let counts = 0;
    const { page } = hoverPage({ region: 1, wide: 0, blocks: 0, cardText: "PAR-1 Range issue" }, seen);
    const real = page.getByTestId.bind(page);
    (page as unknown as Record<string, unknown>).getByTestId = (id: string) => {
      const node = real(id);
      if (id !== "work-item-preview") return node;
      const delayed = {
        ...node,
        count: async () => {
          counts += 1;
          return counts <= 2 ? 0 : 1;
        },
      } as unknown as Record<string, unknown>;
      delayed.first = () => delayed;
      return delayed as unknown as ReturnType<typeof real>;
    };
    const card = await new WebNewDriver(page).hoverCardRead("Range issue");
    expect(card.text).toBe("PAR-1 Range issue");
    expect(counts).toBe(3);
  });

  it("reports empty fields and skips the dismiss when the hover opens no card", async () => {
    const seen: string[] = [];
    const { page, keys } = hoverPage({ region: 1, wide: 0, blocks: 0, cardText: null }, seen);
    const card = await new WebNewDriver(page).hoverCardRead("Undated issue");
    expect(card).toEqual({ text: "", priorityIcon: "", dateColor: "" });
    expect(seen).toEqual(["row"]);
    expect(keys).toEqual([]);
  });
});

describe("web_new layout switch settle", () => {
  const PREFS_URL = "https://api.test/api/workspaces/w/projects/p/user-properties/";

  function switchPage(active: string): PageStub {
    return pageOf({
      role: () => locator({ count: async () => 0 }),
      testId: (id) =>
        id === "layout-switcher-segmented"
          ? locator({
              locator: () =>
                locator({
                  count: async () => 1,
                  getAttribute: async () => active,
                }),
            })
          : locator(),
    });
  }

  function fakeResponse(method: string, url: string, body: string | null): never {
    return {
      request: () => ({ method: () => method, postData: () => body }),
      url: () => url,
    } as never;
  }

  it("awaits the display PATCH round-trip on a changed switch", async () => {
    const stub = switchPage("list");

    await new WebNewDriver(stub.page).layoutsSwitchTo("kanban");

    expect(stub.responses).toHaveLength(1);
    const predicate = stub.responses[0]?.predicate as (response: never) => boolean;
    expect(predicate(fakeResponse("PATCH", PREFS_URL, '{"display_filters":{"layout":"kanban"}}'))).toBe(true);
    expect(predicate(fakeResponse("PATCH", PREFS_URL, '{"filters":{"kanban_filters":{}}}'))).toBe(false);
    expect(predicate(fakeResponse("GET", PREFS_URL, null))).toBe(false);
    expect(predicate(fakeResponse("PATCH", "https://api.test/api/workspaces/w/projects/p/issues/", "{}"))).toBe(false);
  });

  it("skips the PATCH wait on a no-op switch", async () => {
    const stub = switchPage("kanban");

    await new WebNewDriver(stub.page).layoutsSwitchTo("kanban");

    expect(stub.responses).toHaveLength(0);
  });

  it("tolerates a changed switch that never PATCHes display prefs", async () => {
    const stub = switchPage("list");
    const armed = stub.page.waitForResponse.bind(stub.page);
    stub.page.waitForResponse = (async (predicate: (response: never) => unknown, options: unknown) => {
      await armed(predicate, options);
      throw new Error("Timeout 30000ms exceeded.");
    }) as Page["waitForResponse"];

    await expect(new WebNewDriver(stub.page).layoutsSwitchTo("kanban")).resolves.toBeUndefined();

    expect(stub.responses).toHaveLength(1);
  });
});
