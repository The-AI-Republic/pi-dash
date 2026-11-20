// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver history/tool/notice methods (NEWFRONT-607,
// AGT-044/045/046/058/061, NEWPIDASH-337). A stub Page stands in for
// Playwright: the assertions are about the selectors each read uses and
// the shapes each probe reports.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
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
    isVisible: async () => true,
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

describe("history surface reads", () => {
  // NEWPIDASH-337: the history surface is the Chat History header button
  // plus the dialog it opens — no persistent rail. The button carries the
  // surface label, the header link the New-chat entry, and the dialog the
  // link-only thread rows.
  function historyPage(layout: StubLocator, dialog: StubLocator): Page {
    return pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-layout"]') return layout;
        if (selector === '[data-testid="assistant-history-dialog"]') return dialog;
        return locator({ count: async () => 0 });
      },
    });
  }

  function layoutWith(button: StubLocator | null, newChat: StubLocator | null): StubLocator {
    return locator({
      getByRole: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Chat History") {
          return button ?? locator({ count: async () => 0 });
        }
        if (role === "link" && name === "New chat") {
          return newChat ?? locator({ count: async () => 0 });
        }
        return locator({ count: async () => 0 });
      },
    });
  }

  function historyButton(text: string | null): StubLocator {
    return locator({ textContent: async () => text, click: async () => undefined });
  }

  function historyLinkRow(): StubLocator {
    return locator({ evaluate: async (fn) => fn({ tagName: "A" } as unknown as Element) });
  }

  function openDialog(): StubLocator {
    return locator({
      isVisible: async () => false,
      waitFor: async () => undefined,
      locator: (inner: string) =>
        inner === "nav a" ? locator({ count: async () => 2, nth: () => historyLinkRow() }) : locator(),
    });
  }

  it("reads the Chat History button label", async () => {
    const page = historyPage(layoutWith(historyButton(" Chat History "), null), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(page).assistantSidebarHeader()).resolves.toBe("Chat History");
  });

  it("reads null when the button is missing, blank or unreadable", async () => {
    const missing = historyPage(layoutWith(null, null), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(missing).assistantSidebarHeader()).resolves.toBeNull();
    const blank = historyPage(layoutWith(historyButton("  "), null), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(blank).assistantSidebarHeader()).resolves.toBeNull();
    const unreadable = historyPage(layoutWith(historyButton(null), null), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(unreadable).assistantSidebarHeader()).resolves.toBeNull();
  });

  it("reports the header New-chat entry and the dialog thread rows by tag", async () => {
    const newChat = locator({ evaluate: async (fn) => fn({ tagName: "A" } as unknown as Element) });
    const dialog = openDialog();
    const page = historyPage(layoutWith(historyButton("Chat History"), newChat), dialog);
    await expect(new WebNewDriver(page).assistantSidebarRowKinds()).resolves.toEqual({
      newChat: "a",
      rows: ["a", "a"],
    });
  });

  it("reports an empty surface when the layout is absent", async () => {
    const page = historyPage(locator({ count: async () => 0 }), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(page).assistantSidebarRowKinds()).resolves.toEqual({
      newChat: null,
      rows: [],
    });
  });

  it("reports no rows without opening the dialog where the button is absent", async () => {
    const newChat = locator({ evaluate: async (fn) => fn({ tagName: "A" } as unknown as Element) });
    const dialog = locator({
      locator: () => {
        throw new Error("dialog must not open without the history button");
      },
    });
    const page = historyPage(layoutWith(null, newChat), dialog);
    await expect(new WebNewDriver(page).assistantSidebarRowKinds()).resolves.toEqual({
      newChat: "a",
      rows: [],
    });
  });

  it("reads dialog button labels, or nothing when the dialog is closed", async () => {
    const open = historyPage(
      layoutWith(historyButton("Chat History"), null),
      locator({ locator: () => locator({ allInnerTexts: async () => [] }) })
    );
    await expect(new WebNewDriver(open).assistantSidebarButtons()).resolves.toEqual([]);
    const closed = historyPage(layoutWith(historyButton("Chat History"), null), locator({ count: async () => 0 }));
    await expect(new WebNewDriver(closed).assistantSidebarButtons()).resolves.toEqual([]);
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

  it("also scans an open history dialog", async () => {
    // NEWPIDASH-337: the dialog is portaled outside the layout, so the
    // negative scan has to cover it while it is open.
    const dialog = locator({
      isVisible: async () => true,
      locator: (inner: string) =>
        inner === "button, a"
          ? locator({
              count: async () => 2,
              nth: (i: number) => locator({ textContent: async () => (i === 0 ? "Rename thread" : "Beta") }) as never,
            })
          : locator(),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-layout"]') {
          return locator({ locator: () => locator({ count: async () => 0 }) });
        }
        if (selector === '[data-testid="assistant-history-dialog"]') return dialog;
        return locator({ count: async () => 0 });
      },
    });
    await expect(new WebNewDriver(page).assistantThreadManagementControls()).resolves.toEqual(["rename thread"]);
  });
});
