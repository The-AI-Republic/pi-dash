// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver permissions methods (NEWFRONT-658,
// ISS-222–223). A stub Page stands in for Playwright: each case pins the
// list quick-add scope, the detail title editability, and the comment
// overflow menu keys the permissions scenario drives.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  click?: (options?: unknown) => Promise<void>;
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEditable?: () => Promise<boolean>;
  allTextContents?: () => Promise<string[]>;
  waitFor?: (options?: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByLabel?: (label: string) => Locator;
  locator?: (selector: string) => Locator;
  first?: () => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    click: async () => undefined,
    count: async () => 1,
    isVisible: async () => true,
    isEditable: async () => true,
    allTextContents: async () => [],
    waitFor: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    getByLabel: () => self,
    locator: () => self,
    first: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

interface PageParts {
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByLabel?: (label: string) => Locator;
  locator?: (selector: string) => Locator;
  keyboardPress?: (key: string) => Promise<void>;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    getByLabel: (label: string) => parts.getByLabel?.(label) ?? locator(),
    locator: (selector: string) => parts.locator?.(selector) ?? locator(),
    keyboard: { press: parts.keyboardPress ?? (async () => undefined) },
  } as unknown as Page;
}

describe("permissions surfaces", () => {
  it("scopes the quick-add read to the list container", async () => {
    const calls: string[] = [];
    const page = pageOf({
      getByTestId: (id: string | RegExp) => {
        calls.push(`testid:${String(id)}`);
        return locator({
          getByRole: (role: string, roleOptions?: unknown) => {
            calls.push(`role:${role}:${JSON.stringify(roleOptions)}`);
            return locator();
          },
        });
      },
    });
    await expect(new WebNewDriver(page).projectQuickAddVisible()).resolves.toBe(true);
    expect(calls).toContain("testid:quick-add");
    expect(calls).toContain('role:button:{"name":"New work item"}');
  });

  it("reads a missing or hidden quick-add as absent", async () => {
    const never = () =>
      locator({
        waitFor: async () => {
          throw new Error("timed out");
        },
      });
    const missing = pageOf({ getByTestId: () => locator({ getByRole: () => never() }) });
    await expect(new WebNewDriver(missing).projectQuickAddVisible()).resolves.toBe(false);
    const hidden = pageOf({ getByTestId: () => locator({ getByRole: () => never() }) });
    await expect(new WebNewDriver(hidden).projectQuickAddVisible()).resolves.toBe(false);
  });

  it("reads the title field editability", async () => {
    const editable = pageOf({ getByLabel: () => locator({ isEditable: async () => true }) });
    await expect(new WebNewDriver(editable).issueTitleInputEnabled()).resolves.toBe(true);
    const locked = pageOf({ getByLabel: () => locator({ isEditable: async () => false }) });
    await expect(new WebNewDriver(locked).issueTitleInputEnabled()).resolves.toBe(false);
    const bare = pageOf({ getByLabel: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(bare).issueTitleInputEnabled()).resolves.toBe(false);
  });

  it("maps comment menu titles back to stable keys", async () => {
    const pressed: string[] = [];
    const card = locator({
      locator: () => locator({ count: async () => 1 }),
    });
    const page = pageOf({
      locator: (selector: string) => (selector === "#comment-c1" ? card : locator()),
      getByRole: (role: string) =>
        role === "menuitem"
          ? locator({ allTextContents: async () => [" Edit ", "Copy link", "Fold comment", "Delete"] })
          : locator(),
      keyboardPress: async (key: string) => void pressed.push(key),
    });
    await expect(new WebNewDriver(page).rulesCommentMenuOptions("c1")).resolves.toEqual([
      "edit",
      "copy_link",
      "fold",
      "delete",
    ]);
    expect(pressed).toEqual(["Escape"]);
  });

  it("maps the access switch and unfold titles", async () => {
    const page = pageOf({
      getByRole: (role: string) =>
        role === "menuitem"
          ? locator({ allTextContents: async () => ["Switch to private comment", "Unfold comment"] })
          : locator(),
    });
    await expect(new WebNewDriver(page).rulesCommentMenuOptions("c1")).resolves.toEqual(["access_switch", "unfold"]);
  });

  it("reads a trigger-less card as offering nothing", async () => {
    const card = locator({ locator: () => locator({ count: async () => 0 }) });
    const page = pageOf({
      locator: (selector: string) => (selector === "#comment-c1" ? card : locator()),
    });
    await expect(new WebNewDriver(page).rulesCommentMenuOptions("c1")).resolves.toEqual([]);
  });
});
