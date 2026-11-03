// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver edition-stub methods (NEWFRONT-658,
// ISS-234/235/237). A stub Page stands in for Playwright: each case pins
// the sub-issue filter panel, the header key badge, the saved-view open,
// and the gantt presence settle.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  click?: (options?: unknown) => Promise<void>;
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  waitFor?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  locator?: (selector: string) => Locator;
  nth?: (index: number) => Locator;
  first?: () => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    click: async () => undefined,
    count: async () => 1,
    innerText: async () => "",
    waitFor: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    locator: () => self,
    nth: () => self,
    first: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

interface PageParts {
  goto?: (url: string, options?: unknown) => Promise<void>;
  url?: () => string;
  waitForLoadState?: (state: string) => Promise<void>;
  waitForURL?: (pattern: RegExp, options?: unknown) => Promise<void>;
  waitForTimeout?: (ms: number) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  locator?: (selector: string) => Locator;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    goto: parts.goto ?? (async () => undefined),
    url: parts.url ?? (() => "https://parity.test/ws/browse/PAR-1"),
    waitForLoadState: parts.waitForLoadState ?? (async () => undefined),
    waitForURL: parts.waitForURL ?? (async () => undefined),
    waitForTimeout: parts.waitForTimeout ?? (async () => undefined),
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    locator: (selector: string) => parts.locator?.(selector) ?? locator(),
  } as unknown as Page;
}

describe("edition stubs", () => {
  it("opens the sub-issue filters from the parent detail", async () => {
    const gotos: string[] = [];
    const calls: string[] = [];
    const page = pageOf({
      url: () => "https://parity.test/ws/projects/pid",
      goto: async (url: string) => void gotos.push(url),
      getByTestId: (id: string | RegExp) => {
        calls.push(String(id));
        return locator({ click: async () => void calls.push(`click:${String(id)}`) });
      },
    });
    await new WebNewDriver(page).subIssueFiltersOpen("ws", "pid", "parent1");
    expect(gotos).toContain("/ws/projects/pid/issues/parent1");
    expect(calls).toContain("sub-issues-collapsible");
    expect(calls).toContain("click:sub-issue-filters-trigger");
    expect(calls).toContain("sub-issue-filters-panel");
  });

  it("reads the filter panel text normalized", async () => {
    const page = pageOf({
      getByTestId: () => locator({ innerText: async () => "Priority\n  State   Assignee " }),
    });
    await expect(new WebNewDriver(page).subIssueFiltersPanelText()).resolves.toBe("Priority State Assignee");
  });

  it("reads the header key badge", async () => {
    const page = pageOf({
      getByRole: () => locator({ innerText: async () => "  PAR-12 " }),
    });
    await expect(new WebNewDriver(page).detailIdentifierText()).resolves.toBe("PAR-12");
  });

  it("copies the key through the badge click", async () => {
    let clicks = 0;
    const page = pageOf({
      getByRole: () => locator({ click: async () => void (clicks += 1) }),
    });
    await new WebNewDriver(page).detailIdentifierCopy();
    expect(clicks).toBe(1);
  });

  it("opens the saved view address", async () => {
    const gotos: string[] = [];
    const page = pageOf({ goto: async (url: string) => void gotos.push(url) });
    await new WebNewDriver(page).viewsOpenDetail("ws", "pid", "view1");
    expect(gotos).toEqual(["/ws/projects/pid/views/view1"]);
  });

  it("reads a present row as shown even without a bar", async () => {
    const link = locator({ innerText: async () => "WEB-1 First", getAttribute: async () => "issue-row-1" });
    const dateless = pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        return locator({ first: () => locator({ count: async () => 0, boundingBox: async () => null }) });
      },
    });
    await expect(new WebNewDriver(dateless).ganttShowsIssue("First")).resolves.toBe(true);
  });

  it("reads a missing row as absent", async () => {
    // Construct before mocking time: the driver's startedAt initializer
    // consumes a Date.now call at construction.
    const bare = pageOf({
      locator: () => locator({ count: async () => 0 }),
      waitForTimeout: async () => undefined,
    });
    const driver = new WebNewDriver(bare);
    let calls = 0;
    const now = vi.spyOn(Date, "now").mockImplementation(() => {
      calls += 1;
      return calls <= 1 ? 1_000 : 1_000 + 61_000;
    });
    try {
      await expect(driver.ganttShowsIssue("First")).resolves.toBe(false);
    } finally {
      now.mockRestore();
    }
  });
});
