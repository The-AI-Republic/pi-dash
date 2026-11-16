// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycle-transfer methods (NEWFRONT-658,
// ISS-224). A stub Page stands in for Playwright: each case pins the
// transfer prompt, dialog search, option spans, and pick-and-close flow
// the cycle-transfer scenario drives.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  click?: (options?: unknown) => Promise<void>;
  count?: () => Promise<number>;
  allInnerTexts?: () => Promise<string[]>;
  waitFor?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByPlaceholder?: (text: string) => Locator;
  locator?: (selector: string) => Locator;
  first?: () => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    click: async () => undefined,
    count: async () => 1,
    allInnerTexts: async () => [],
    waitFor: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    getByPlaceholder: () => self,
    locator: () => self,
    first: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

function absent(): Locator {
  return locator({ count: async () => 0 });
}

interface PageParts {
  goto?: (url: string, options?: unknown) => Promise<void>;
  waitForTimeout?: (ms: number) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByPlaceholder?: (text: string) => Locator;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    goto: parts.goto ?? (async () => undefined),
    waitForTimeout: parts.waitForTimeout ?? (async () => undefined),
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    getByPlaceholder: (text: string) => parts.getByPlaceholder?.(text) ?? locator(),
  } as unknown as Page;
}

describe("cycle transfer", () => {
  it("opens the cycle page and settles on the layout switcher", async () => {
    const gotos: string[] = [];
    const page = pageOf({
      goto: async (url: string) => void gotos.push(url),
      getByTestId: () => locator({ count: async () => 1 }),
    });
    await new WebNewDriver(page).openCycleIssues("ws", "pid", "cid");
    expect(gotos).toEqual(["/ws/projects/pid/cycles/cid"]);
  });

  it("gives up settling after a minute without throwing", async () => {
    const page = pageOf({
      getByTestId: () => locator({ count: async () => 0 }),
      getByRole: () => locator({ count: async () => 0 }),
    });
    // Construct before mocking time: the driver's startedAt initializer
    // consumes a Date.now call at construction.
    const driver = new WebNewDriver(page);
    let calls = 0;
    const now = vi.spyOn(Date, "now").mockImplementation(() => {
      calls += 1;
      return calls <= 1 ? 1_000 : 1_000 + 61_000;
    });
    try {
      await driver.openCycleIssues("ws", "pid", "cid");
    } finally {
      now.mockRestore();
    }
  });

  it("reads the transfer prompt presence", async () => {
    const present = pageOf({ getByRole: () => locator({ count: async () => 1 }) });
    await expect(new WebNewDriver(present).cycleTransferButtonVisible()).resolves.toBe(true);
    const bare = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(bare).cycleTransferButtonVisible()).resolves.toBe(false);
  });

  it("opens the dialog on the prompt and waits for the search", async () => {
    const calls: string[] = [];
    const page = pageOf({
      getByRole: () => locator({ click: async () => void calls.push("prompt") }),
      getByPlaceholder: (text: string) => {
        calls.push(`wait:${text}`);
        return locator();
      },
    });
    await new WebNewDriver(page).cycleTransferOpen();
    expect(calls).toEqual(["prompt", "wait:Search for a cycle..."]);
  });

  it("reads option names from the name spans only", async () => {
    const options = locator({
      getByRole: () =>
        locator({
          locator: (selector: string) =>
            selector === ":scope > span.truncate"
              ? locator({ allInnerTexts: async () => [" Target ", "", "Other"] })
              : absent(),
        }),
    });
    const page = pageOf({ getByRole: () => options });
    await expect(new WebNewDriver(page).cycleTransferOptionNames()).resolves.toEqual(["Target", "Other"]);
  });

  it("picks the named option and waits for the dialog to close", async () => {
    const calls: string[] = [];
    const options = locator({
      getByRole: (_role: string, roleOptions?: unknown) => {
        calls.push(`pick:${String((roleOptions as { name?: unknown } | undefined)?.name)}`);
        return locator({ click: async () => void calls.push("clicked") });
      },
    });
    const page = pageOf({
      getByRole: () => options,
      getByPlaceholder: () =>
        locator({
          waitFor: async (waitOptions?: unknown) => void calls.push(`wait:${JSON.stringify(waitOptions)}`),
        }),
    });
    await new WebNewDriver(page).cycleTransferPick("Target (1)");
    expect(calls).toContain("pick:/Target \\(1\\)/");
    expect(calls).toContain("clicked");
    expect(calls).toContain(`wait:${JSON.stringify({ state: "detached", timeout: 60_000 })}`);
  });
});
