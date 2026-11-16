// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for the WebDriver board root (NEWFRONT-177, ISS-036):
// opening any HeadlessUI Dialog marks the app container inert +
// aria-hidden, which removes <main> from the accessibility tree and
// blinds page.getByRole("main") for the whole modal-open duration.
// Board reads must scope to CSS "main" (inert-immune) instead. A stub
// Page stands in for Playwright: getByRole resolves to nothing (the
// blind accessibility tree) while locator("main") serves the board.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebDriver } from "./web";

interface StubLocator {
  count?: () => Promise<number>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    getAttribute: async () => null,
    innerText: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    nth: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

/** A locator matching nothing; chained queries stay empty. */
function emptyLocator(): Locator {
  const node: StubLocator = {};
  node.count = async () => 0;
  node.first = () => node;
  node.nth = () => node;
  node.locator = () => node;
  return node as unknown as Locator;
}

/**
 * A stub page behind an open modal: role queries see nothing (inert
 * accessibility tree) while the DOM still holds one flat column outer
 * with a "Todo 2" sticky header and a rendered drop target.
 */
function pageBehindOpenModal(getByRole: ReturnType<typeof vi.fn>): Page {
  const header = locator({ count: async () => 1, innerText: async () => "Todo 2" });
  const inner = locator({ count: async () => 1, getAttribute: async () => "state-uuid__null" });
  const outer = locator({
    locator: (selector: string) => {
      if (selector === ":scope > div.sticky") return header;
      if (selector === ':scope div[id$="__null"]') return inner;
      return emptyLocator();
    },
  });
  const outers = locator({
    count: async () => 1,
    nth: () => outer,
    first: () => outer,
  });
  const main = locator({
    locator: (selector: string) => {
      if (selector === "div.group.relative.flex.flex-shrink-0.flex-col") return outers;
      return emptyLocator();
    },
  });
  return {
    getByRole,
    locator: (selector: string) => (selector === "main" ? main : emptyLocator()),
  } as unknown as Page;
}

describe("board reads behind an open modal", () => {
  it("kanbanColumns resolves the flat columns without touching role queries", async () => {
    const getByRole = vi.fn(() => emptyLocator());
    const driver = new WebDriver(pageBehindOpenModal(getByRole));
    await expect(driver.kanbanColumns()).resolves.toEqual([
      { id: "state-uuid", name: "Todo", count: 2, rendered: true },
    ]);
    expect(getByRole).not.toHaveBeenCalled();
  });

  it("kanbanColumns returns no columns when the board itself is empty", async () => {
    const getByRole = vi.fn(() => emptyLocator());
    const main = locator({ locator: () => emptyLocator() });
    const page = {
      getByRole,
      locator: (selector: string) => (selector === "main" ? main : emptyLocator()),
    } as unknown as Page;
    await expect(new WebDriver(page).kanbanColumns()).resolves.toEqual([]);
    expect(getByRole).not.toHaveBeenCalled();
  });
});
