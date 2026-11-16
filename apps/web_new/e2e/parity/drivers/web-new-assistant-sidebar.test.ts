// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver sidebar/tool/notice methods (NEWFRONT-607,
// AGT-044/045/046/058/061). A stub Page stands in for Playwright: the
// assertions are about the selectors each read uses and the shapes each
// probe reports.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  allInnerTexts?: () => Promise<string[]>;
  evaluate?: (fn: (node: Element) => unknown) => Promise<unknown>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 1,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    allInnerTexts: async () => [],
    evaluate: async () => 0,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    getByRole: () => node,
    getByText: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    goBack: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    url: () => "https://app.test/acme/assistant/t1",
    getByRole: vi.fn(() => locator()),
    getByText: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    route: vi.fn(async () => undefined),
    unroute: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as Page;
}

function transcriptPage(activities: StubLocator[]): Page {
  return pageOf({
    locator: (selector: string) =>
      selector === '[data-testid="assistant-transcript"]'
        ? locator({
            locator: (inner: string) =>
              inner === '[data-testid="assistant-tool-activity"]'
                ? locator({ count: async () => activities.length, nth: (i) => activities[i] as never })
                : locator(),
          })
        : locator(),
  });
}

function toolActivity(text: string, links: { label: string; href: string | null }[]): StubLocator {
  const anchors = links.map((link) =>
    locator({
      innerText: async () => link.label,
      getAttribute: async () => link.href,
    })
  );
  return locator({
    locator: (inner: string) => {
      if (inner === '[data-testid="assistant-tool-text"]') return locator({ innerText: async () => text });
      if (inner === ":scope a") return locator({ count: async () => anchors.length, nth: (i) => anchors[i] as never });
      return locator();
    },
  });
}

describe("tool activity reads", () => {
  it("reads activity text without link labels plus each deep link", async () => {
    const page = transcriptPage([
      toolActivity("Searching", []),
      toolActivity("Found it", [
        { label: "Open thread", href: "/acme/assistant/t1" },
        { label: "View", href: null },
      ]),
    ]);
    await expect(new WebNewDriver(page).assistantToolActivities()).resolves.toEqual([
      { text: "Searching", links: [] },
      {
        text: "Found it",
        links: [
          { label: "Open thread", href: "/acme/assistant/t1" },
          { label: "View", href: "" },
        ],
      },
    ]);
  });

  it("reads no activities when the transcript holds none", async () => {
    const page = transcriptPage([]);
    await expect(new WebNewDriver(page).assistantToolActivities()).resolves.toEqual([]);
  });

  it("clicks the addressed tool link", async () => {
    const clicks: string[] = [];
    const linked = locator({
      locator: (inner: string) =>
        inner === ":scope a"
          ? locator({
              count: async () => 2,
              nth: (i) =>
                locator({
                  click: async () => {
                    clicks.push(`link-${i}`);
                  },
                }) as never,
            })
          : locator(),
    });
    const page = transcriptPage([locator(), linked]);
    await new WebNewDriver(page).assistantClickToolLink(1, 1);
    expect(clicks).toEqual(["link-1"]);
  });
});

describe("notice reads", () => {
  function noticePage(texts: string[]): Page {
    const lines = texts.map((text) => locator({ innerText: async () => text }));
    return pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({
              locator: (inner: string) =>
                inner === '[data-testid="assistant-notice-line"]'
                  ? locator({ count: async () => lines.length, nth: (i) => lines[i] as never })
                  : locator(),
            })
          : locator(),
    });
  }

  it("reads one line per server and skips blank lines", async () => {
    const page = noticePage([" Tool server a was unavailable. ", "  ", "Tool server b was unavailable."]);
    await expect(new WebNewDriver(page).assistantNoticeLines()).resolves.toEqual([
      "Tool server a was unavailable.",
      "Tool server b was unavailable.",
    ]);
  });

  it("reads no lines when the transcript holds no notices", async () => {
    await expect(new WebNewDriver(noticePage([])).assistantNoticeLines()).resolves.toEqual([]);
  });
});

describe("sidebar identity reads", () => {
  function railPage(rail: StubLocator): Page {
    return pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-rail"]' ? rail : locator({ count: async () => 0 }),
    });
  }

  it("reads the rail brand header", async () => {
    const page = railPage(
      locator({
        locator: (inner: string) =>
          inner === '[data-testid="assistant-rail-header"]'
            ? locator({ textContent: async () => " Pi Dash AI " })
            : locator(),
      })
    );
    await expect(new WebNewDriver(page).assistantSidebarHeader()).resolves.toBe("Pi Dash AI");
  });

  it("reads null when the header is missing, blank or unreadable", async () => {
    const missing = railPage(locator({ locator: () => locator({ count: async () => 0 }) }));
    await expect(new WebNewDriver(missing).assistantSidebarHeader()).resolves.toBeNull();
    const blank = railPage(locator({ locator: () => locator({ textContent: async () => "  " }) }));
    await expect(new WebNewDriver(blank).assistantSidebarHeader()).resolves.toBeNull();
    const unreadable = railPage(locator({ locator: () => locator({ textContent: async () => null }) }));
    await expect(new WebNewDriver(unreadable).assistantSidebarHeader()).resolves.toBeNull();
  });

  it("reports the New-chat entry and thread rows by tag", async () => {
    const nodes = [
      { tag: "A", text: "New chat" },
      { tag: "A", text: "Beta" },
      { tag: "A", text: "Alpha" },
    ].map((row) =>
      locator({
        evaluate: async (fn) => fn({ tagName: row.tag } as unknown as Element),
        textContent: async () => row.text,
      })
    );
    const page = railPage(
      locator({
        locator: (inner: string) =>
          inner === ":scope a, :scope button"
            ? locator({ count: async () => nodes.length, nth: (i) => nodes[i] as never })
            : locator(),
      })
    );
    await expect(new WebNewDriver(page).assistantSidebarRowKinds()).resolves.toEqual({
      newChat: "a",
      rows: ["a", "a"],
    });
  });

  it("reports an empty rail when the sidebar is absent", async () => {
    const page = railPage(locator({ count: async () => 0 }));
    await expect(new WebNewDriver(page).assistantSidebarRowKinds()).resolves.toEqual({
      newChat: null,
      rows: [],
    });
  });

  it("reads sidebar button labels, or nothing when the sidebar is absent", async () => {
    const page = railPage(locator({ locator: () => locator({ allInnerTexts: async () => ["Rename"] }) }));
    await expect(new WebNewDriver(page).assistantSidebarButtons()).resolves.toEqual(["Rename"]);
    const missing = railPage(locator({ count: async () => 0 }));
    await expect(new WebNewDriver(missing).assistantSidebarButtons()).resolves.toEqual([]);
  });
});

describe("thread management reads", () => {
  function layoutPage(candidates: (string | null)[] | null): Page {
    if (candidates === null) {
      return pageOf({ locator: () => locator({ count: async () => 0 }) });
    }
    const nodes = candidates.map((text) => locator({ textContent: async () => text }));
    return pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-layout"]'
          ? locator({
              locator: (inner: string) =>
                inner === "button, a"
                  ? locator({ count: async () => nodes.length, nth: (i) => nodes[i] as never })
                  : locator(),
            })
          : locator(),
    });
  }

  it("names rename, archive and delete affordances and skips the rest", async () => {
    const page = layoutPage(["Rename thread", "Archive", "DELETE", "New chat", "Beta", null]);
    await expect(new WebNewDriver(page).assistantThreadManagementControls()).resolves.toEqual([
      "rename thread",
      "archive",
      "delete",
    ]);
  });

  it("reads no controls when the layout is absent or clean", async () => {
    await expect(new WebNewDriver(layoutPage(null)).assistantThreadManagementControls()).resolves.toEqual([]);
    await expect(
      new WebNewDriver(layoutPage(["New chat", "Beta"])).assistantThreadManagementControls()
    ).resolves.toEqual([]);
  });
});
