// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver mobile-header methods (NEWFRONT-658,
// ISS-003). A stub Page stands in for Playwright: each case pins the
// compact-bar hooks the mobile scenarios read, so a markup change that
// breaks them fails here before the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  isVisible?: () => Promise<boolean>;
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
    click: async () => undefined,
    waitFor: async () => undefined,
    isVisible: async () => true,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node({ count: async () => 0 }),
    getByTestId: () => node({ count: async () => 0 }),
    getByRole: () => node({ count: async () => 0 }),
    filter: () => self as unknown as Locator,
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

function menuItem(label: string, onClick?: () => void): Locator {
  return node({
    innerText: async () => label,
    click: async () => onClick?.(),
  });
}

interface PageStub {
  page: Page;
  testIds: string[];
  keys: string[];
}

function pageOf(
  parts: {
    testId?: (id: string) => Locator;
    role?: (role: string, options?: unknown) => Locator;
    locator?: (selector: string) => Locator;
  } = {}
): PageStub {
  const state: PageStub = { page: undefined as unknown as Page, testIds: [], keys: [] };
  state.page = {
    getByTestId: (id: string) => {
      state.testIds.push(id);
      return parts.testId?.(id) ?? node({ count: async () => 0 });
    },
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? node({ count: async () => 0 }),
    locator: (selector: string) => parts.locator?.(selector) ?? node({ count: async () => 0 }),
    keyboard: {
      press: async (key: string) => {
        state.keys.push(key);
      },
    },
  } as unknown as Page;
  return state;
}

/** The compact bar with a layout trigger and an analytics entry. */
function mobileHeader(clicks: string[], analytics: boolean): Locator {
  return node({
    getByTestId: (id: string) => {
      if (id === "mobile-layout-trigger") return node({ click: async () => void clicks.push("trigger") });
      if (id === "issues-analytics-entry") {
        return analytics ? node() : node({ count: async () => 0 });
      }
      return node({ count: async () => 0 });
    },
  });
}

describe("web_new mobile header", () => {
  it("maps the offered layout menu labels to keys in order", async () => {
    const clicks: string[] = [];
    const stub = pageOf({
      testId: (id: string) =>
        id === "issues-mobile-header" ? mobileHeader(clicks, true) : node({ count: async () => 0 }),
      role: (role: string) =>
        role === "menuitem"
          ? collection([menuItem("List"), menuItem("Board"), menuItem("Calendar")])
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsMobileOfferedLayouts()).resolves.toEqual([
      "list",
      "kanban",
      "calendar",
    ]);
    expect(clicks).toEqual(["trigger"]);
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("throws on an unrecognised mobile layout label", async () => {
    const stub = pageOf({
      testId: (id: string) => (id === "issues-mobile-header" ? mobileHeader([], true) : node({ count: async () => 0 })),
      role: (role: string) =>
        role === "menuitem" ? collection([menuItem("List"), menuItem("Timeline 3D")]) : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(stub.page).layoutsMobileOfferedLayouts()).rejects.toThrow(
      '[parity] unrecognised mobile layout "Timeline 3D".'
    );
    expect(stub.keys).toEqual(["Escape"]);
  });

  it("switches layouts through the mobile menu and settles", async () => {
    const clicks: string[] = [];
    const stub = pageOf({
      testId: (id: string) => {
        if (id === "issues-mobile-header") return mobileHeader(clicks, true);
        if (id === "issues-calendar") return node();
        return node({ count: async () => 0 });
      },
      role: (role: string) =>
        role === "menuitem"
          ? collection([menuItem("List"), menuItem("Board"), menuItem("Calendar", () => clicks.push("calendar"))])
          : node({ count: async () => 0 }),
    });
    await new WebNewDriver(stub.page).layoutsMobileSwitchTo("calendar");
    expect(clicks).toEqual(["trigger", "calendar"]);
    expect(stub.testIds).toContain("issues-calendar");
  });

  it("reads the analytics entry off the compact bar only", async () => {
    const present = pageOf({
      // A page-level entry must not satisfy the read: the desktop header
      // renders the same node, hidden by viewport classes.
      testId: (id: string) => (id === "issues-mobile-header" ? mobileHeader([], true) : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(present.page).layoutsMobileAnalyticsVisible()).resolves.toBe(true);

    const missing = pageOf({
      testId: (id: string) =>
        id === "issues-mobile-header" ? mobileHeader([], false) : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(missing.page).layoutsMobileAnalyticsVisible()).resolves.toBe(false);
  });

  it("reads hidden group-by rows as disabled display options", async () => {
    const panel = (cycle: boolean, module: boolean): Locator =>
      node({
        getByTestId: (id: string) =>
          id === "display-section-group"
            ? node({
                getByRole: (_role: string, options?: unknown) => {
                  const name = (options as { name?: unknown } | undefined)?.name;
                  const shown = (name === "Cycle" && cycle) || (name === "Module" && module);
                  return shown ? node() : node({ count: async () => 0 });
                },
              })
            : node({ count: async () => 0 }),
      });
    const stubFor = (cycle: boolean, module: boolean): PageStub =>
      pageOf({
        testId: (id: string) => {
          if (id === "issues-mobile-header") return node({ getByTestId: () => node() });
          if (id === "display-options-panel") return panel(cycle, module);
          return node({ count: async () => 0 });
        },
      });

    const both = stubFor(true, true);
    await expect(new WebNewDriver(both.page).layoutsMobileDisplayCycleModuleDisabled()).resolves.toEqual({
      cycleDisabled: false,
      moduleDisabled: false,
    });
    expect(both.keys).toEqual(["Escape"]);

    const none = stubFor(false, false);
    await expect(new WebNewDriver(none.page).layoutsMobileDisplayCycleModuleDisabled()).resolves.toEqual({
      cycleDisabled: true,
      moduleDisabled: true,
    });

    const split = stubFor(false, true);
    await expect(new WebNewDriver(split.page).layoutsMobileDisplayCycleModuleDisabled()).resolves.toEqual({
      cycleDisabled: true,
      moduleDisabled: false,
    });
  });
});
