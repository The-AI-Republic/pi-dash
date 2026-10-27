// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver shared layout-surface reads
// (NEWFRONT-658, ISS-001/002). A stub Page stands in for Playwright: each
// case pins the header-switcher and layout-root hooks the shared
// scenarios read, so a markup change that breaks them fails here before
// the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  getAttribute?: (name: string) => Promise<string | null>;
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
    getAttribute: async () => null,
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

function pageOf(
  parts: {
    testId?: (id: string) => Locator;
    role?: (role: string, options?: unknown) => Locator;
    locator?: (selector: string) => Locator;
  } = {}
): Page {
  return {
    getByTestId: (id: string) => parts.testId?.(id) ?? node({ count: async () => 0 }),
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? node({ count: async () => 0 }),
    locator: (selector: string) => parts.locator?.(selector) ?? node({ count: async () => 0 }),
  } as unknown as Page;
}

/** A segmented switcher offering `keys` with `active` marked. */
function switcher(keys: string[], active: string | null): Locator {
  return node({
    locator: (selector: string) => {
      if (selector === "[data-layout]") {
        return collection(keys.map((key) => node({ getAttribute: async () => key })));
      }
      if (selector === '[data-layout][data-active="true"]') {
        return active === null ? node({ count: async () => 0 }) : node({ getAttribute: async () => active });
      }
      return node({ count: async () => 0 });
    },
  });
}

describe("web_new shared layout surfaces", () => {
  it("reads offered layouts off the segmented switcher in order", async () => {
    const page = pageOf({
      testId: (id: string) =>
        id === "layout-switcher-segmented"
          ? switcher(["list", "kanban", "calendar", "spreadsheet", "gantt_chart"], "list")
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsOfferedLayouts()).resolves.toEqual([
      "list",
      "kanban",
      "calendar",
      "spreadsheet",
      "gantt_chart",
    ]);
  });

  it("throws on an unrecognised offered layout key", async () => {
    const page = pageOf({
      testId: (id: string) =>
        id === "layout-switcher-segmented" ? switcher(["list", "timeline-3d"], "list") : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsOfferedLayouts()).rejects.toThrow(
      '[parity] unrecognised layout key "timeline-3d".'
    );
  });

  it("reads the active layout off the marked segmented choice", async () => {
    const page = pageOf({
      testId: (id: string) =>
        id === "layout-switcher-segmented"
          ? switcher(["list", "calendar"], "calendar")
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsActiveLayout()).resolves.toEqual("calendar");
  });

  it("falls back to the profile trigger without a segmented switcher", async () => {
    const page = pageOf({
      role: (role: string) =>
        role === "button"
          ? node({ getAttribute: async () => "Issue layout, current Board" })
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsActiveLayout()).resolves.toEqual("kanban");
  });

  it("throws on an unrecognised profile layout label", async () => {
    const page = pageOf({
      role: (role: string) =>
        role === "button"
          ? node({ getAttribute: async () => "Issue layout, current Wall" })
          : node({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).layoutsActiveLayout()).rejects.toThrow(
      '[parity] unrecognised layout trigger label "Issue layout, current Wall".'
    );
  });

  it("reads list visibility off either surface", async () => {
    const profile = pageOf({
      testId: (id: string) => (id === "profile-list" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(profile).layoutsListVisible()).resolves.toBe(true);

    const project = pageOf({
      testId: (id: string) => (id === "list-group-header" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(project).layoutsListVisible()).resolves.toBe(true);

    const neither = pageOf({ testId: () => node({ count: async () => 0 }) });
    await expect(new WebNewDriver(neither).layoutsListVisible()).resolves.toBe(false);
  });

  it("reads kanban visibility off either surface", async () => {
    const profile = pageOf({
      testId: (id: string) => (id === "profile-board" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(profile).layoutsKanbanVisible()).resolves.toBe(true);

    const project = pageOf({
      testId: (id: string) => (id === "kanban-layout" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(project).layoutsKanbanVisible()).resolves.toBe(true);

    const neither = pageOf({ testId: () => node({ count: async () => 0 }) });
    await expect(new WebNewDriver(neither).layoutsKanbanVisible()).resolves.toBe(false);
  });

  it("settles each lazy layout on its own root after switching", async () => {
    const waited: string[] = [];
    const page = pageOf({
      testId: (id: string) =>
        node({
          waitFor: async () => void waited.push(id),
        }),
      locator: (selector: string) => {
        if (selector.startsWith("[data-layout=")) return node();
        return node({ count: async () => 0 });
      },
    });
    const driver = new WebNewDriver(page);
    await driver.layoutsSwitchTo("spreadsheet");
    await driver.layoutsSwitchTo("kanban");
    await driver.layoutsSwitchTo("gantt_chart");
    expect(waited).toEqual(["spreadsheet-scroller", "kanban-layout", "gantt-layout"]);
  });

  it("reads gantt visibility off the gantt root", async () => {
    const shown = pageOf({ testId: (id: string) => (id === "gantt-layout" ? node() : node({ count: async () => 0 })) });
    await expect(new WebNewDriver(shown).layoutsGanttVisible()).resolves.toBe(true);

    const hidden = pageOf({ testId: () => node({ count: async () => 0 }) });
    await expect(new WebNewDriver(hidden).layoutsGanttVisible()).resolves.toBe(false);
  });
});
