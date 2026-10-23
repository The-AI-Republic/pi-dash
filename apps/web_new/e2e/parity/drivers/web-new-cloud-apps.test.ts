// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cloud Apps catalog methods (area
// gate NEWFRONT-662, CLOUD-021–031). A stub Page stands in for
// Playwright: the assertions pin the selectors and branches against
// the openhub components — never a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  click?: (options?: unknown) => Promise<void>;
  innerText?: () => Promise<string>;
  first?: () => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 0,
    click: async () => undefined,
    innerText: async () => "",
    isVisible: async () => false,
    isDisabled: async () => false,
    inputValue: async () => "",
    fill: async () => undefined,
    pressSequentially: async () => undefined,
    getAttribute: async () => null,
    waitFor: async () => undefined,
    evaluateAll: async () => [],
    ...overrides,
  };
  const chain: Record<string, unknown> = {
    first: () => node,
    nth: () => locator(),
    locator: () => locator(),
    getByRole: () => locator(),
    getByTestId: () => locator(),
    getByPlaceholder: () => locator(),
    getByLabel: () => locator(),
    filter: () => locator(),
  };
  for (const [name, value] of Object.entries(chain)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByRole: vi.fn(() => locator()),
    getByTestId: vi.fn(() => locator()),
    getByPlaceholder: vi.fn(() => locator()),
    getByLabel: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    ...overrides,
  } as unknown as Page;
}

function cardOf(facts: {
  name: string;
  text: string;
  actions?: string[];
  iconSrc?: string | null;
  letter?: string;
  onAction?: (action: string) => void;
  disabledActions?: string[];
}): Locator {
  const actionLocators = new Map<string, Locator>();
  for (const action of facts.actions ?? []) {
    actionLocators.set(
      action,
      locator({
        count: async () => 1,
        click: async () => facts.onAction?.(action),
        isDisabled: async () => (facts.disabledActions ?? []).includes(action),
        innerText: async () => action,
      })
    );
  }
  const collection = locator({
    count: async () => actionLocators.size,
    nth: (index: number) => [...actionLocators.values()][index] ?? locator(),
  });
  return locator({
    count: async () => 1,
    innerText: async () => facts.text,
    getByTestId: (id: string) => {
      if (id === "app-card-name") return locator({ count: async () => 1, innerText: async () => facts.name });
      if (id === "app-card-icon") {
        if (facts.iconSrc === null || facts.iconSrc === undefined) return locator();
        return locator({
          count: async () => 1,
          isVisible: async () => true,
          getAttribute: async () => facts.iconSrc,
        });
      }
      if (id === "app-card-tile") {
        if (facts.letter === undefined) return locator();
        return locator({ count: async () => 1, innerText: async () => facts.letter ?? "" });
      }
      return locator();
    },
    getByRole: (role: string, options?: { name?: string }) => {
      if (role !== "button") return locator();
      if (options?.name === undefined) return collection;
      return actionLocators.get(options.name) ?? locator();
    },
  });
}

function dialogOf(overrides: Partial<StubLocator> = {}): Locator {
  const dialog = locator({ count: async () => 1, isVisible: async () => true, ...overrides });
  // The driver scopes role=dialog through .filter().first() (toasts share
  // the role); the stub dialog survives its own filter.
  (dialog as unknown as StubLocator).filter = () => dialog;
  return dialog;
}

function cardsPage(entries: Array<{ name: string; card: Locator }>): Page {
  const main = locator({
    getByTestId: (id: string) => {
      if (id === "apps-catalog-grid")
        return locator({
          count: async () => 1,
          getByTestId: (inner: string) =>
            inner === "app-card" ? locator({ count: async () => entries.length }) : locator(),
        });
      if (id === "app-card")
        return locator({
          count: async () => entries.length,
          nth: (index: number) => entries[index]?.card ?? locator(),
        });
      if (id === "app-card-name")
        return locator({
          count: async () => entries.length,
          nth: (index: number) => locator({ innerText: async () => entries[index]?.name ?? "" }),
        });
      return locator();
    },
  });
  return pageOf({ locator: () => main });
}

describe("cloud apps sidebar", () => {
  it("reads the Apps entry href and icon", async () => {
    const link = locator({
      count: async () => 1,
      getAttribute: async () => "/ws-one/apps/",
      locator: (selector: string) => (selector === "svg" ? locator({ count: async () => 1 }) : locator()),
    });
    const seen: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        seen.push(selector);
        return selector === '#main-sidebar a[href$="/apps/"]' ? link : locator();
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAppsSidebarHref()).resolves.toBe("/ws-one/apps/");
    await expect(driver.cloudAppsSidebarIconPresent()).resolves.toBe(true);
    expect(seen).toContain('#main-sidebar a[href$="/apps/"]');
  });

  it("reads null/false when the entry is absent", async () => {
    const driver = new WebNewDriver(pageOf());
    await expect(driver.cloudAppsSidebarHref()).resolves.toBeNull();
    await expect(driver.cloudAppsSidebarIconPresent()).resolves.toBe(false);
  });

  it("opens the catalog from the sidebar link", async () => {
    const clicked = vi.fn(async () => undefined);
    const waited: string[] = [];
    const page = pageOf({
      locator: () => locator({ click: clicked }),
      waitForURL: vi.fn(async (pattern: string) => void waited.push(pattern)),
    });
    await new WebNewDriver(page).cloudAppsOpenFromSidebar();
    expect(clicked).toHaveBeenCalledTimes(1);
    expect(waited).toEqual(["**/apps*"]);
  });

  it("proves the quick-create control opens the shared dialog", async () => {
    const clicked = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "sidebar-quick-create" ? locator({ count: async () => 1, click: clicked }) : locator(),
      getByPlaceholder: () => locator({ waitFor: async () => undefined }),
    });
    await expect(new WebNewDriver(page).cloudAppsSidebarCreateOpensDialog()).resolves.toBe(true);
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("reports false when quick-create is absent or the dialog never opens", async () => {
    await expect(new WebNewDriver(pageOf()).cloudAppsSidebarCreateOpensDialog()).resolves.toBe(false);
    const page = pageOf({
      getByTestId: () => locator({ count: async () => 1 }),
      getByPlaceholder: () =>
        locator({
          waitFor: async () => {
            throw new Error("never");
          },
        }),
    });
    await expect(new WebNewDriver(page).cloudAppsSidebarCreateOpensDialog()).resolves.toBe(false);
  });
});

describe("cloud apps search", () => {
  it("fills, types, reads and reports the search box", async () => {
    const filled: string[] = [];
    const pressed: string[] = [];
    const box = locator({
      fill: vi.fn(async (value: string) => void filled.push(value)),
      click: vi.fn(async () => undefined),
      pressSequentially: vi.fn(async (text: string) => void pressed.push(text)),
      inputValue: async () => filled.join(""),
      isVisible: async () => true,
    });
    const seen: unknown[] = [];
    const page = pageOf({
      locator: () =>
        locator({
          getByPlaceholder: (...args: unknown[]) => {
            seen.push(args);
            return box;
          },
        }),
    });
    const driver = new WebNewDriver(page);
    await driver.cloudAppsSearchFill("not");
    await driver.cloudAppsSearchTypeText("es");
    await expect(driver.cloudAppsSearchValue()).resolves.toBe("not");
    await expect(driver.cloudAppsSearchVisible()).resolves.toBe(true);
    expect(seen).toContainEqual(["Search apps", { exact: true }]);
    expect(pressed).toEqual(["es"]);
  });

  it("reads empty/hidden when the box is missing", async () => {
    const missing = (): Promise<never> => Promise.reject(new Error("missing"));
    const page = pageOf({
      locator: () =>
        locator({
          getByPlaceholder: () => locator({ inputValue: missing, isVisible: missing }),
        }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAppsSearchValue()).resolves.toBe("");
    await expect(driver.cloudAppsSearchVisible()).resolves.toBe(false);
  });
});

describe("cloud apps cards", () => {
  const fresh = {
    name: "Fresh App",
    card: cardOf({ name: "Fresh App", text: "Fresh App Install", actions: ["Install"], letter: "F" }),
  };
  const ready = {
    name: "Ready App",
    card: cardOf({
      name: "Ready App",
      text: "Ready App Installed Uninstall",
      actions: ["Uninstall"],
      iconSrc: "https://proxy.example.com/icon.png",
      letter: "R",
    }),
  };
  const inline = {
    name: "Inline Icon",
    card: cardOf({
      name: "Inline Icon",
      text: "Inline Icon Install",
      actions: ["Install"],
      iconSrc: "data:image/svg+xml,abc",
      // No tile: the icon card reports the name initial.
    }),
  };
  const needsAuth = {
    name: "Manual CRM",
    card: cardOf({
      name: "Manual CRM",
      text: "Manual CRM Needs connection Connect",
      actions: ["Connect"],
      letter: "M",
    }),
  };

  it("lists card names in order", async () => {
    const driver = new WebNewDriver(cardsPage([fresh, ready]));
    await expect(driver.cloudAppsCardNames()).resolves.toEqual(["Fresh App", "Ready App"]);
  });

  it("reads badges, actions, icon stages and letters", async () => {
    const driver = new WebNewDriver(cardsPage([fresh, ready, inline, needsAuth]));
    await expect(driver.cloudAppsCards()).resolves.toEqual([
      { name: "Fresh App", badge: null, actions: ["Install"], icon: "letter", letter: "F" },
      { name: "Ready App", badge: "installed", actions: ["Uninstall"], icon: "proxied", letter: "R" },
      { name: "Inline Icon", badge: null, actions: ["Install"], icon: "inline", letter: "I" },
      { name: "Manual CRM", badge: "needs-connection", actions: ["Connect"], icon: "letter", letter: "M" },
    ]);
  });

  it("clicks a card action by exact card and action names", async () => {
    const acted: string[] = [];
    const entry = {
      name: "Fresh App",
      card: cardOf({
        name: "Fresh App",
        text: "Fresh App Install",
        actions: ["Install"],
        onAction: (action) => void acted.push(action),
      }),
    };
    await new WebNewDriver(cardsPage([entry])).cloudAppsClickCardAction("Fresh App", "Install");
    expect(acted).toEqual(["Install"]);
  });

  it("throws for unknown cards and missing actions", async () => {
    const driver = new WebNewDriver(cardsPage([fresh]));
    await expect(driver.cloudAppsClickCardAction("Missing", "Install")).rejects.toThrow('no Apps card named "Missing"');
    await expect(driver.cloudAppsCardActionDisabled("Missing", "Install")).rejects.toThrow(
      'no Apps card named "Missing"'
    );
    await expect(driver.cloudAppsCardActionDisabled("Fresh App", "Uninstall")).rejects.toThrow(
      'no Uninstall action on "Fresh App"'
    );
  });

  it("reads action disabled state", async () => {
    const busy = {
      name: "Busy",
      card: cardOf({ name: "Busy", text: "Busy Install", actions: ["Install"], disabledActions: ["Install"] }),
    };
    const driver = new WebNewDriver(cardsPage([fresh, busy]));
    await expect(driver.cloudAppsCardActionDisabled("Fresh App", "Install")).resolves.toBe(false);
    await expect(driver.cloudAppsCardActionDisabled("Busy", "Install")).resolves.toBe(true);
  });

  it("reports catalog visibility by card presence", async () => {
    await expect(new WebNewDriver(cardsPage([fresh])).cloudAppsCatalogVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(cardsPage([])).cloudAppsCatalogVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(pageOf()).cloudAppsCatalogVisible()).resolves.toBe(false);
  });
});

describe("cloud apps pager", () => {
  it("turns pages and reads the indicator", async () => {
    const clicked: string[] = [];
    const page = pageOf({
      locator: () =>
        locator({
          getByRole: (_role: string, options?: { name?: string }) =>
            locator({ click: async () => void clicked.push(options?.name ?? "?") }),
          getByTestId: (id: string) =>
            id === "apps-page-indicator"
              ? locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "Page 2" })
              : locator(),
        }),
    });
    const driver = new WebNewDriver(page);
    await driver.cloudAppsNextPage();
    await driver.cloudAppsPrevPage();
    await expect(driver.cloudAppsPageIndicator()).resolves.toBe("Page 2");
    await expect(driver.cloudAppsPagerVisible()).resolves.toBe(true);
    expect(clicked).toEqual(["Next", "Previous"]);
  });

  it("reads null/hidden when the pager is absent", async () => {
    const driver = new WebNewDriver(pageOf());
    await expect(driver.cloudAppsPageIndicator()).resolves.toBeNull();
    await expect(driver.cloudAppsPagerVisible()).resolves.toBe(false);
  });
});

describe("cloud apps dialog", () => {
  function dialogPage(dialog: Locator): Page {
    return pageOf({ getByRole: () => dialog });
  }

  it("reads visibility, title and setup href", async () => {
    const dialog = dialogOf({
      getByRole: (child: string, childOptions?: { name?: string }) => {
        if (child === "heading") return locator({ innerText: async () => "Connect Manual CRM" });
        if (child === "link" && childOptions?.name === "Get credentials")
          return locator({ count: async () => 1, getAttribute: async () => "https://crm.example.com/keys" });
        return locator();
      },
    });
    const driver = new WebNewDriver(dialogPage(dialog));
    await expect(driver.cloudAppsDialogVisible()).resolves.toBe(true);
    await expect(driver.cloudAppsDialogTitle()).resolves.toBe("Connect Manual CRM");
    await expect(driver.cloudAppsDialogSetupHref()).resolves.toBe("https://crm.example.com/keys");
  });

  it("reads fields, instructions and errors", async () => {
    const label = (text: string, fieldId: string) =>
      locator({ innerText: async () => text, getAttribute: async (name: string) => (name === "for" ? fieldId : null) });
    const dialog = dialogOf({
      locator: (selector: string) => {
        if (selector === "label")
          return locator({
            count: async () => 2,
            nth: (index: number) => (index === 0 ? label("API key", "f1") : label("Region", "f2")),
          });
        if (selector === "#f1")
          return locator({ getAttribute: async (name: string) => (name === "type" ? "password" : "sk-...") });
        if (selector === "#f2")
          return locator({ getAttribute: async (name: string) => (name === "type" ? "text" : "us") });
        if (selector === "p:not([role='alert'])")
          return locator({ count: async () => 1, innerText: async () => "Paste your API key below." });
        if (selector === 'p[role="alert"]')
          return locator({ count: async () => 1, nth: () => locator({ innerText: async () => "Enter a key." }) });
        return locator();
      },
    });
    const driver = new WebNewDriver(dialogPage(dialog));
    await expect(driver.cloudAppsDialogFields()).resolves.toEqual([
      { label: "API key", inputType: "password", placeholder: "sk-..." },
      { label: "Region", inputType: "text", placeholder: "us" },
    ]);
    await expect(driver.cloudAppsDialogInstructions()).resolves.toBe("Paste your API key below.");
    await expect(driver.cloudAppsDialogErrors()).resolves.toEqual(["Enter a key."]);
  });

  it("fills, submits and cancels", async () => {
    const filled: Array<[string, string]> = [];
    const clicked: string[] = [];
    const dialog = dialogOf({
      getByLabel: (label: string) => locator({ fill: async (value: string) => void filled.push([label, value]) }),
      getByRole: (_role: string, options?: { name?: string }) =>
        locator({ click: async () => void clicked.push(options?.name ?? "?") }),
    });
    const driver = new WebNewDriver(dialogPage(dialog));
    await driver.cloudAppsDialogFill("API key", "sk-abcd");
    await driver.cloudAppsDialogSubmit();
    await driver.cloudAppsDialogCancel();
    expect(filled).toEqual([["API key", "sk-abcd"]]);
    expect(clicked).toEqual(["Connect", "Cancel"]);
  });

  it("reads dialog input values", async () => {
    const dialog = dialogOf({ locator: () => locator({ evaluateAll: async () => ["", ""] }) });
    const driver = new WebNewDriver(dialogPage(dialog));
    await expect(driver.cloudAppsDialogInputValues()).resolves.toEqual(["", ""]);
  });
});

describe("cloud apps toggle and body", () => {
  it("reads state, text and flips the toggle", async () => {
    const clicked = vi.fn(async () => undefined);
    const toggle = locator({ count: async () => 1, click: clicked, getAttribute: async () => "true" });
    const page = pageOf({
      getByRole: (role: string) => {
        if (role !== "region") return locator();
        return locator({
          count: async () => 1,
          innerText: async () => "Agent tools Use apps in the assistant",
          getByRole: () => toggle,
        });
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAppsToggleState()).resolves.toBe(true);
    await expect(driver.cloudAppsToggleText()).resolves.toBe("Agent tools Use apps in the assistant");
    await driver.cloudAppsToggle();
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("reads null/empty when the toggle is absent", async () => {
    const driver = new WebNewDriver(pageOf());
    await expect(driver.cloudAppsToggleState()).resolves.toBeNull();
    await expect(driver.cloudAppsToggleText()).resolves.toBe("");
  });

  it("reads collapsed body text and clicks retry", async () => {
    const clicked = vi.fn(async () => undefined);
    const page = pageOf({
      locator: () =>
        locator({
          innerText: async () => "Apps\n  No apps available",
          getByRole: () => locator({ click: clicked }),
        }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAppsBodyText()).resolves.toBe("Apps No apps available");
    await driver.cloudAppsClickRetry();
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("reads the first non-blank intro paragraph", async () => {
    const page = pageOf({
      locator: () =>
        locator({
          locator: () =>
            locator({
              count: async () => 3,
              nth: (index: number) =>
                locator({ innerText: async () => ["", "Apps you install are yours.", "more"][index] ?? "" }),
            }),
        }),
    });
    await expect(new WebNewDriver(page).cloudAppsIntroText()).resolves.toBe("Apps you install are yours.");
  });
});
