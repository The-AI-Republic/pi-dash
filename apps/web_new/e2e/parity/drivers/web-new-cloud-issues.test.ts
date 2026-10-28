// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the issues cloud-presence driver methods (NEWFRONT-984):
// the cloud bulk toolbar reads and actions plus the timeline dependency
// counts and drag-to-link. A stub Page pins the selectors and the pointer
// order without bringing up the parity stack.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  check?: (options?: unknown) => Promise<void>;
  fill?: (text: string) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options?: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    check: async () => undefined,
    fill: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    ...overrides,
  };
  node.first = () => node;
  if (node.nth === undefined) node.nth = () => node;
  if (node.filter === undefined) node.filter = () => node;
  if (node.getByRole === undefined) node.getByRole = () => node;
  if (node.getByTestId === undefined) node.getByTestId = () => node;
  return node as unknown as Locator;
}

interface PageState {
  testIds: string[];
  roleCalls: Array<{ role: string; name: unknown; exact: unknown }>;
  dialogNames: unknown[];
  selectors: string[];
  mouse: string[];
  waits: unknown[];
}

function pageOf(
  hooks: {
    onTestId?: (state: PageState, id: string) => Locator;
    onRole?: (state: PageState, role: string, options: unknown) => Locator | undefined;
    onLocator?: (state: PageState, selector: string) => Locator;
    clicks?: () => void;
    fills?: (text: string) => void;
  } = {}
): { page: Page; state: PageState } {
  const state: PageState = { testIds: [], roleCalls: [], dialogNames: [], selectors: [], mouse: [], waits: [] };
  const button = locator({
    click: async () => {
      hooks.clicks?.();
    },
    check: async () => {
      hooks.clicks?.();
    },
    fill: async (text: string) => {
      hooks.fills?.(text);
    },
  });
  const dialog = locator({
    getByRole: ((role: string, options?: { name?: unknown; exact?: unknown }) => {
      state.roleCalls.push({ role, name: options?.name, exact: options?.exact });
      return button;
    }) as StubLocator["getByRole"],
    waitFor: (async (options?: unknown) => {
      state.waits.push(options);
    }) as StubLocator["waitFor"],
  });
  const toolbar = locator({
    getByRole: ((role: string, options?: { name?: unknown; exact?: unknown }) => {
      state.roleCalls.push({ role, name: options?.name, exact: options?.exact });
      return button;
    }) as StubLocator["getByRole"],
  });
  const page = {
    getByTestId: (id: string) => {
      state.testIds.push(id);
      if (hooks.onTestId !== undefined) return hooks.onTestId(state, id);
      if (id === "bulk-toolbar") return toolbar;
      return locator({ count: async () => 0, isVisible: async () => false });
    },
    getByRole: (role: string, options?: { name?: unknown; exact?: unknown }) => {
      state.roleCalls.push({ role, name: options?.name, exact: options?.exact });
      if (hooks.onRole !== undefined) {
        const override = hooks.onRole(state, role, options);
        if (override !== undefined) return override;
      }
      if (role === "dialog") {
        state.dialogNames.push(options?.name);
        return dialog;
      }
      return button;
    },
    locator: (selector: string) => {
      state.selectors.push(selector);
      if (hooks.onLocator !== undefined) return hooks.onLocator(state, selector);
      return locator();
    },
    getByPlaceholder: () => locator(),
    mouse: {
      down: async () => {
        state.mouse.push("down");
      },
      up: async () => {
        state.mouse.push("up");
      },
    },
  } as unknown as Page;
  return { page, state };
}

describe("web_new issues cloud presence", () => {
  it("reads the functional toolbar, not the OSS banner", async () => {
    const { page, state } = pageOf();
    await expect(new WebNewDriver(page).bulkToolbarVisible()).resolves.toBe(true);
    expect(state.testIds).toEqual(["bulk-toolbar"]);
  });

  it("reports no toolbar when nothing renders", async () => {
    const { page } = pageOf({
      onTestId: () => locator({ count: async () => 0, isVisible: async () => false }),
    });
    await expect(new WebNewDriver(page).bulkToolbarVisible()).resolves.toBe(false);
  });

  it("deletes through the toolbar confirm", async () => {
    const clicks = vi.fn();
    const { page, state } = pageOf({ clicks });
    await new WebNewDriver(page).bulkToolbarDelete();
    expect(state.testIds).toEqual(["bulk-toolbar"]);
    expect(state.roleCalls).toEqual([
      { role: "button", name: "Delete", exact: true },
      { role: "dialog", name: "Delete selected work items?", exact: undefined },
      { role: "button", name: "Delete", exact: true },
    ]);
    expect(clicks).toHaveBeenCalledTimes(2);
    expect(state.waits).toHaveLength(1);
  });

  it("archives through the toolbar button", async () => {
    const clicks = vi.fn();
    const { page, state } = pageOf({ clicks });
    await new WebNewDriver(page).bulkToolbarArchive();
    expect(state.testIds).toEqual(["bulk-toolbar"]);
    expect(state.roleCalls).toEqual([{ role: "button", name: "Archive", exact: true }]);
    expect(clicks).toHaveBeenCalledTimes(1);
  });

  it("counts dependency arrows and handles", async () => {
    const { page, state } = pageOf({
      onTestId: (_state, id) => {
        if (id === "gantt-dependency-arrow") return locator({ count: async () => 2 });
        if (id === "gantt-dependency-handle") return locator({ count: async () => 3 });
        return locator({ count: async () => 0 });
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.ganttDependencyArrowCount()).resolves.toBe(2);
    await expect(driver.ganttDependencyHandleCount()).resolves.toBe(3);
    expect(state.testIds).toEqual(["gantt-dependency-arrow", "gantt-dependency-handle"]);
  });

  it("opens the sub-issue filter menu and reads its panel", async () => {
    const { page, state } = pageOf({
      onTestId: (_state, id) => {
        if (id === "sub-issue-filters-panel") return locator({ innerText: async () => "Team projects" });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.subIssueFiltersOpen("ws", "p", "i");
    await expect(driver.subIssueFiltersPanelText()).resolves.toBe("Team projects");
    expect(state.testIds).toEqual(["sub-issue-filters-trigger", "sub-issue-filters-panel", "sub-issue-filters-panel"]);
  });

  it("updates the selection priority through the edit dialog", async () => {
    const clicks = vi.fn();
    const { page, state } = pageOf({
      clicks,
      onRole: (pageState, role) => {
        if (role !== "listbox") return undefined;
        return locator({
          getByRole: ((optionRole: string, options?: { name?: unknown; exact?: unknown }) => {
            pageState.roleCalls.push({ role: optionRole, name: options?.name, exact: options?.exact });
            return locator({
              click: async () => {
                clicks();
              },
            });
          }) as StubLocator["getByRole"],
        });
      },
    });
    await new WebNewDriver(page).bulkToolbarUpdatePriority("High");
    expect(state.testIds).toEqual(["bulk-toolbar"]);
    expect(state.roleCalls).toEqual([
      { role: "button", name: "Edit properties", exact: true },
      { role: "dialog", name: "Edit selected work items", exact: undefined },
      { role: "checkbox", name: "Priority", exact: true },
      { role: "button", name: "None", exact: true },
      { role: "listbox", name: "Priorities", exact: undefined },
      { role: "option", name: "High", exact: true },
      { role: "button", name: "Apply", exact: true },
    ]);
    // Edit trigger, arm checkbox, picker trigger, option, apply.
    expect(clicks).toHaveBeenCalledTimes(5);
    expect(state.waits).toHaveLength(1);
  });

  it("subscribes through the toolbar button", async () => {
    const clicks = vi.fn();
    const { page, state } = pageOf({ clicks });
    await new WebNewDriver(page).bulkToolbarSubscribe();
    expect(state.testIds).toEqual(["bulk-toolbar"]);
    expect(state.roleCalls).toEqual([{ role: "button", name: "Subscribe", exact: true }]);
    expect(clicks).toHaveBeenCalledTimes(1);
  });

  it("selects the first N rows, excluding the group header", async () => {
    const picked: number[] = [];
    const { page, state } = pageOf({
      onRole: (pageState, role, options) => {
        expect(role).toBe("checkbox");
        expect(String((options as { name: unknown }).name)).toBe("/^Select (?!all\\b)/");
        return locator({
          count: async () => 4,
          nth: (index: number) => {
            picked.push(index);
            return locator({
              click: async () => undefined,
            });
          },
        });
      },
    });
    await new WebNewDriver(page).selectFirstRows(2);
    expect(picked).toEqual([0, 1]);
    expect(state.roleCalls).toHaveLength(1);
  });

  it("creates an epic through the widget action", async () => {
    const clicks = vi.fn();
    const fills = vi.fn();
    const { page, state } = pageOf({ clicks, fills });
    await new WebNewDriver(page).epicCreateViaWidget("Big rock");
    expect(state.roleCalls).toEqual([
      { role: "button", name: "Create epic", exact: true },
      { role: "dialog", name: "Create epic", exact: undefined },
      { role: "textbox", name: "Epic name", exact: true },
      { role: "button", name: "Create epic", exact: true },
    ]);
    expect(fills).toHaveBeenCalledWith("Big rock");
    expect(clicks).toHaveBeenCalledTimes(2);
    expect(state.waits).toHaveLength(1);
  });

  it("renames an epic through its relation-row menu", async () => {
    const clicks = vi.fn();
    const fills = vi.fn();
    const { page, state } = pageOf({ clicks, fills });
    await new WebNewDriver(page).epicRenameRelationRow("Big rock", "Bigger rock");
    expect(state.roleCalls).toEqual([
      { role: "button", name: "Actions for Big rock", exact: true },
      { role: "menuitem", name: "Edit", exact: true },
      { role: "dialog", name: "Edit epic", exact: undefined },
      { role: "textbox", name: "Epic name", exact: true },
      { role: "button", name: "Save epic", exact: true },
    ]);
    expect(fills).toHaveBeenCalledWith("Bigger rock");
    expect(clicks).toHaveBeenCalledTimes(3);
    expect(state.waits).toHaveLength(1);
  });

  it("picks a work-item type in the create modal", async () => {
    const clicks = vi.fn();
    const { page, state } = pageOf({
      clicks,
      onRole: (pageState, role) => {
        if (role !== "listbox") return undefined;
        return locator({
          getByRole: ((optionRole: string, options?: { name?: unknown; exact?: unknown }) => {
            pageState.roleCalls.push({ role: optionRole, name: options?.name, exact: options?.exact });
            return locator({
              click: async () => {
                clicks();
              },
            });
          }) as StubLocator["getByRole"],
        });
      },
    });
    await new WebNewDriver(page).modalSelectIssueType("Bug");
    expect(state.roleCalls).toEqual([
      { role: "dialog", name: undefined, exact: undefined },
      { role: "button", name: "No type", exact: true },
      { role: "listbox", name: "Issue types", exact: undefined },
      { role: "option", name: "Bug", exact: true },
    ]);
    expect(clicks).toHaveBeenCalledTimes(2);
  });

  it("links blocks by dragging a handle onto a lane", async () => {
    const hovers: string[] = [];
    const { page, state } = pageOf({
      onLocator: (_state, selector) =>
        locator({
          hover: async () => {
            hovers.push(selector);
          },
        }),
    });
    await new WebNewDriver(page).ganttLinkBlocks("from-1", "to-2");
    expect(state.selectors).toEqual([
      '[data-testid="gantt-dependency-handle"][data-source-id="from-1"]',
      '[data-gantt-lane][data-lane-id="to-2"]',
    ]);
    expect(hovers).toEqual(state.selectors);
    expect(state.mouse).toEqual(["down", "up"]);
  });
});
