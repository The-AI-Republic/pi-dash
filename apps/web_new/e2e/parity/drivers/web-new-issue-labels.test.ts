// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver issue-labels methods (NEWFRONT-658,
// ISS-226–227). A stub Page stands in for Playwright: each case pins the
// sidebar Labels row, the project-labels option list, and the option-or-
// empty settle the settings-crud scenario drives.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  click?: (options?: unknown) => Promise<void>;
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  waitFor?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  locator?: (selector: string) => Locator;
  first?: () => Locator;
  or?: (other: Locator) => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    click: async () => undefined,
    count: async () => 1,
    innerText: async () => "",
    allInnerTexts: async () => [],
    waitFor: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    locator: () => self,
    first: () => self,
    or: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

interface PageParts {
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  locator?: (selector: string) => Locator;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    locator: (selector: string) => parts.locator?.(selector) ?? locator(),
  } as unknown as Page;
}

describe("issue labels picker", () => {
  it("opens the picker from the Labels row and settles on options or empty", async () => {
    const calls: string[] = [];
    const row = locator({ getByRole: () => locator({ click: async () => void calls.push("trigger") }) });
    const page = pageOf({
      getByRole: () => row,
      getByTestId: (id: string | RegExp) => {
        calls.push(`wait:${String(id)}`);
        return locator();
      },
      locator: (selector: string) => {
        calls.push(`list:${selector}`);
        return locator();
      },
    });
    await new WebNewDriver(page).issueLabelsOpenPicker();
    expect(calls).toContain("trigger");
    expect(calls).toContain("wait:label-picker-search");
    expect(calls).toContain('list:ul[aria-label="Project labels"]');
    expect(calls).toContain("wait:label-picker-empty");
  });

  it("reads option names in order, dropping blanks", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'ul[aria-label="Project labels"]'
          ? locator({
              getByTestId: () => locator({ allInnerTexts: async () => ["  Alpha ", "", "Beta"] }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).issueLabelsOptionTexts()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("reads the Labels row text normalized", async () => {
    const row = locator({ innerText: async () => "Labels\n  Alpha   Beta " });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).issueLabelsRowText()).resolves.toBe("Labels Alpha Beta");
  });

  it("waits for chips when the trigger promises a selection", async () => {
    const calls: string[] = [];
    const row = locator({ innerText: async () => "Labels 1 selected" });
    const page = pageOf({
      getByRole: () => row,
      locator: (selector: string) => {
        calls.push(selector);
        return locator();
      },
    });
    await expect(new WebNewDriver(page).issueLabelsRowText()).resolves.toBe("Labels 1 selected");
    expect(calls).toContain('ul[aria-label="Selected labels"]');
  });

  it("skips the chips wait when nothing is selected", async () => {
    const calls: string[] = [];
    const row = locator({ innerText: async () => "Labels Select labels" });
    const page = pageOf({
      getByRole: () => row,
      locator: (selector: string) => {
        calls.push(selector);
        return locator();
      },
    });
    await expect(new WebNewDriver(page).issueLabelsRowText()).resolves.toBe("Labels Select labels");
    expect(calls).toEqual([]);
  });

  it("reads an unreadable row as empty", async () => {
    const row = locator({
      waitFor: async () => undefined,
      innerText: async () => {
        throw new Error("detached");
      },
    });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).issueLabelsRowText()).resolves.toBe("");
  });
});
