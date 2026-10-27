// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver peek reads (NEWFRONT-658, ISS-165/172).
// A stub Page stands in for Playwright: each case pins the preview
// shell, mode switch, header actions, and error hooks the peek
// scenarios drive, so a markup change that breaks them fails here
// before the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  isVisible?: () => Promise<boolean>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  first?: () => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    innerText: async () => "",
    allTextContents: async () => [],
    getAttribute: async () => null,
    boundingBox: async () => null,
    click: async () => undefined,
    waitFor: async () => undefined,
    isVisible: async () => true,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node({ count: async () => 0 }),
    getByTestId: () => node({ count: async () => 0 }),
    getByRole: () => node({ count: async () => 0 }),
    first: () => self as unknown as Locator,
  };
  for (const [name, value] of Object.entries(chain)) {
    if (self[name as keyof StubNode] === undefined) {
      (self as Record<string, unknown>)[name] = value;
    }
  }
  return self as unknown as Locator;
}

function absent(): Locator {
  return node({ count: async () => 0 });
}

interface PageParts {
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  locator?: (selector: string) => Locator;
  keyboardPress?: (key: string) => Promise<void>;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByTestId: (id: string) => parts.testId?.(id) ?? absent(),
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? absent(),
    locator: (selector: string) => parts.locator?.(selector) ?? absent(),
    keyboard: { press: (key: string) => parts.keyboardPress?.(key) ?? Promise.resolve() },
  } as unknown as Page;
}

describe("DetailPeekDriver", () => {
  it("reads the identifier from the preview header", async () => {
    const preview = node({
      getByRole: (role: string) => (role === "button" ? node({ innerText: async () => "PAR-3" }) : absent()),
    });
    const page = pageOf({ testId: (id: string) => (id === "issue-preview" ? preview : absent()) });
    await expect(new WebNewDriver(page).peekIdentifier()).resolves.toBe("PAR-3");
    await expect(new WebNewDriver(pageOf()).peekIdentifier()).resolves.toBeNull();
  });

  it("reads close visibility from the preview shell", async () => {
    const shown = pageOf({
      testId: (id: string) => (id === "issue-preview" ? node({ getByRole: () => node({}) }) : absent()),
    });
    await expect(new WebNewDriver(shown).peekCloseVisible()).resolves.toBe(true);
    const hidden = pageOf({
      testId: (id: string) =>
        id === "issue-preview" ? node({ getByRole: () => node({ isVisible: async () => false }) }) : absent(),
    });
    await expect(new WebNewDriver(hidden).peekCloseVisible()).resolves.toBe(false);
  });

  it("switches the peek mode through the toggle menu", async () => {
    const calls: string[] = [];
    const preview = node({
      getByTestId: (id: string) =>
        id === "peek-mode-trigger"
          ? node({
              click: async () => {
                calls.push("click:trigger");
              },
            })
          : absent(),
    });
    const page = pageOf({
      testId: (id: string) => (id === "issue-preview" ? preview : absent()),
      role: (role: string, options?: unknown) => {
        if (role !== "menuitem") return absent();
        const want = (options as { name?: unknown } | undefined)?.name;
        return node({
          click: async () => {
            calls.push(`click:${String(want)}`);
          },
        });
      },
    });
    await new WebNewDriver(page).setPeekMode("Full Screen");
    expect(calls).toEqual(["click:trigger", "click:/^Full Screen$/i"]);
  });

  it("reads the panel box and null when absent", async () => {
    const page = pageOf({
      testId: (id: string) =>
        id === "issue-preview"
          ? node({ boundingBox: async () => ({ x: 900, y: 0, width: 360, height: 800 }) })
          : absent(),
    });
    await expect(new WebNewDriver(page).peekPanelBox()).resolves.toEqual({ x: 900, y: 0, width: 360, height: 800 });
    await expect(new WebNewDriver(pageOf()).peekPanelBox()).resolves.toBeNull();
  });

  it("clicks the peek copy control", async () => {
    let clicked = false;
    const preview = node({
      getByRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name;
        if (role === "button" && want === "Copy work item link") {
          return node({
            click: async () => {
              clicked = true;
            },
          });
        }
        return absent();
      },
    });
    const page = pageOf({ testId: (id: string) => (id === "issue-preview" ? preview : absent()) });
    await new WebNewDriver(page).copyPeekLink();
    expect(clicked).toBe(true);
  });

  it("reads the full-screen href and null when absent", async () => {
    const page = pageOf({
      testId: (id: string) =>
        id === "peek-fullscreen-link" ? node({ getAttribute: async () => "/ws/browse/PAR-3" }) : absent(),
    });
    await expect(new WebNewDriver(page).peekFullScreenHref()).resolves.toBe("/ws/browse/PAR-3");
    await expect(new WebNewDriver(pageOf()).peekFullScreenHref()).resolves.toBeNull();
  });

  it("lists peek quick actions and dismisses the menu", async () => {
    const keys: string[] = [];
    const preview = node({
      getByTestId: (id: string) => (id === "detail-menu-trigger" ? node({}) : absent()),
    });
    const page = pageOf({
      testId: (id: string) => {
        if (id === "issue-preview") return preview;
        return absent();
      },
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? node({ allTextContents: async () => ["Delete", "Archive"] })
          : absent(),
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await expect(new WebNewDriver(page).peekQuickActionNames()).resolves.toEqual(["Delete", "Archive"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("reads the peek error title and null otherwise", async () => {
    const missing = pageOf({
      testId: (id: string) =>
        id === "issue-preview"
          ? node({ innerText: async () => "Work item does not exist\nIt may have been deleted" })
          : absent(),
    });
    await expect(new WebNewDriver(missing).peekErrorTitle()).resolves.toBe("Work item does not exist");
    const healthy = pageOf({
      testId: (id: string) => (id === "issue-preview" ? node({ innerText: async () => "Some title" }) : absent()),
    });
    await expect(new WebNewDriver(healthy).peekErrorTitle()).resolves.toBeNull();
    await expect(new WebNewDriver(pageOf()).peekErrorTitle()).resolves.toBeNull();
  });
});
