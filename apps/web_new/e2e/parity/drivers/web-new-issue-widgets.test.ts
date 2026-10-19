// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver issue-widget methods (NEWFRONT-650,
// ISS-187–191 and ISS-205–206). A stub Page stands in for Playwright; each
// assertion covers the selectors, waits and values the driver reads back.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  allInnerTexts?: () => Promise<string[]>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  fill?: (value: string) => Promise<void>;
  inputValue?: () => Promise<string>;
  hover?: (options?: unknown) => Promise<void>;
  setInputFiles?: (files: unknown) => Promise<void>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    allTextContents: async () => [],
    allInnerTexts: async () => [],
    evaluate: async () => null,
    fill: async () => undefined,
    inputValue: async () => "",
    hover: async () => undefined,
    setInputFiles: async () => undefined,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByLabel: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByTestId: () => locator(),
    getByRole: () => locator(),
    getByPlaceholder: () => locator(),
    locator: () => locator(),
    waitForTimeout: async () => undefined,
    waitForFunction: async () => undefined,
    evaluate: async () => null,
    ...overrides,
  } as unknown as Page;
}

describe("widget rows", () => {
  it("reads each panel row's collapsed text", async () => {
    const rowText = [" plan.png 1 KB ", "", "notes.pdf\n2 MB"];
    const rows = locator({
      count: async () => rowText.length,
      nth: (index: number) => locator({ innerText: async () => rowText[index] ?? "" }) as never,
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-widget-rows="Attachments"]') return locator({ locator: () => rows });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).widgetRowNames("Attachments")).resolves.toEqual([
      "plan.png 1 KB",
      "notes.pdf 2 MB",
    ]);
  });

  it("opens an expanded section without clicking, and a collapsed one with a click", async () => {
    const clicks: string[] = [];
    let expanded: string | null = "true";
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-widget-header="Attachments"]') {
          return locator({
            getAttribute: async (name: string) => (name === "aria-expanded" ? expanded : null),
            click: async () => {
              clicks.push("Attachments");
              expanded = "false";
            },
          });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.widgetExpanded("Attachments")).resolves.toBe(true);
    await driver.openWidgetSection("Attachments");
    expect(clicks).toEqual([]);
    expanded = "false";
    await expect(driver.widgetExpanded("Attachments")).resolves.toBe(false);
    await driver.openWidgetSection("Attachments");
    expect(clicks).toEqual(["Attachments"]);
    await driver.toggleWidgetSection("Attachments");
    expect(clicks).toEqual(["Attachments", "Attachments"]);
  });

  it("reports a section with no header as collapsed", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).widgetExpanded("Code reviews")).resolves.toBe(false);
  });

  it("lists row actions and clicks the requested one", async () => {
    const clicked: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[role="menuitem"]:visible') {
          return locator({
            allTextContents: async () => [" Delete ", "  ", "Download"],
            filter: (options: unknown) => {
              const needle = (options as { hasText: string }).hasText;
              return locator({ click: async () => void clicked.push(needle) });
            },
          });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.widgetRowActionNames("Attachments", "plan.png")).resolves.toEqual(["Delete", "Download"]);
    await driver.clickWidgetRowAction("Attachments", "plan.png", "Delete");
    expect(clicked).toEqual(["Delete"]);
  });

  it("confirms a download opens in a tab and returns its bytes", async () => {
    const chunks = [Buffer.from("abc"), Buffer.from("def")];
    const download = {
      suggestedFilename: () => "plan.png",
      createReadStream: async () =>
        (async function* () {
          yield* chunks;
        })(),
    };
    const popup = { close: async () => undefined };
    const page = pageOf({
      waitForEvent: async (name: string) => (name === "download" ? download : popup),
    });
    const opened = await new WebNewDriver(page).downloadWidgetRowFile("Attachments", "plan.png");
    expect(opened.fileName).toBe("plan.png");
    expect(opened.bytes.toString()).toBe("abcdef");
  });

  it("rejects a download that opened no tab", async () => {
    const download = {
      suggestedFilename: () => "plan.png",
      createReadStream: async () => null,
    };
    const page = pageOf({
      waitForEvent: async (name: string) => (name === "download" ? download : Promise.reject(new Error("no popup"))),
    });
    await expect(new WebNewDriver(page).downloadWidgetRowFile("Attachments", "plan.png")).rejects.toThrow(
      "downloaded without opening a tab"
    );
  });

  it("rejects a download whose stream is empty", async () => {
    const download = { suggestedFilename: () => "plan.png", createReadStream: async () => null };
    const popup = { close: async () => undefined };
    const page = pageOf({
      waitForEvent: async (name: string) => (name === "download" ? download : popup),
    });
    await expect(new WebNewDriver(page).downloadWidgetRowFile("Attachments", "plan.png")).rejects.toThrow(
      "produced no bytes"
    );
  });

  it("reads and confirms the app dialog, skipping empty notification roots", async () => {
    const clicked: string[] = [];
    const dialogs = locator({
      count: async () => 2,
      nth: (index: number) => {
        if (index === 0) return locator({ innerText: async () => "   " });
        return locator({
          innerText: async () => "Delete attachment\nThe file plan will be permanently removed.",
          locator: () => locator({ count: async () => 0 }),
          getByRole: () =>
            locator({
              first: () => locator({ click: async () => void clicked.push("Delete") }) as never,
            }),
        });
      },
    });
    const page = pageOf({ locator: () => dialogs });
    const driver = new WebNewDriver(page);
    await expect(driver.confirmModalTitle()).resolves.toBe("Delete attachment");
    await expect(driver.confirmModalText()).resolves.toContain("permanently removed");
    await driver.confirmModal("Delete");
    expect(clicked).toEqual(["Delete"]);
  });

  it("returns null dialogs and refuses to confirm when none is open", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    await expect(driver.confirmModalTitle()).resolves.toBeNull();
    await expect(driver.confirmModalText()).resolves.toBeNull();
    await expect(driver.confirmModal("Delete")).rejects.toThrow("no open confirmation dialog");
  });
});

describe("attachment upload", () => {
  it("uses a file chooser when the picker opens one", async () => {
    const setFiles = vi.fn(async () => undefined);
    const page = pageOf({
      waitForEvent: async () => ({ setFiles }),
      getByRole: () => locator(),
    });
    const bytes = Buffer.from("png");
    await new WebNewDriver(page).uploadAttachment({ name: "plan.png", mime: "image/png", bytes });
    expect(setFiles).toHaveBeenCalledWith([{ name: "plan.png", mimeType: "image/png", buffer: bytes }]);
  });

  it("falls back to the hidden file input when no chooser event fires", async () => {
    const setInputFiles = vi.fn(async () => undefined);
    const page = pageOf({
      waitForEvent: async () => Promise.reject(new Error("no chooser")),
      locator: (selector: string) => {
        if (selector === 'input[type="file"]') return locator({ setInputFiles });
        return locator();
      },
    });
    const bytes = Buffer.from("pdf");
    await new WebNewDriver(page).uploadAttachment({ name: "notes.pdf", mime: "application/pdf", bytes });
    expect(setInputFiles).toHaveBeenCalledWith([{ name: "notes.pdf", mimeType: "application/pdf", buffer: bytes }]);
  });
});

describe("code reviews", () => {
  it("reports a mounted and visible section, and absence", async () => {
    const visible = pageOf({ locator: () => locator({ count: async () => 1, isVisible: async () => true }) });
    await expect(new WebNewDriver(visible).codeReviewsVisible()).resolves.toBe(true);
    const hidden = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(hidden).codeReviewsVisible()).resolves.toBe(false);
  });

  it("maps each linked review's badge, title, href and target", async () => {
    const anchors = [
      { badge: "Merged", title: "octocat/hello#2", href: "https://github.com/o/h/pull/2", target: "_blank" },
      { badge: "Open", title: "octocat/hello#1", href: "https://github.com/o/h/pull/1", target: "_blank" },
    ];
    const section = {
      querySelectorAll: () =>
        anchors.map((anchor) => ({
          closest: () => ({ firstElementChild: { textContent: anchor.badge } }),
          innerText: anchor.title,
          getAttribute: (name: string) => (name === "href" ? anchor.href : name === "target" ? anchor.target : null),
        })),
    };
    vi.stubGlobal("document", { querySelector: () => section });
    try {
      const page = pageOf({ evaluate: async (fn: unknown) => (fn as () => unknown)() });
      await expect(new WebNewDriver(page).codeReviewLinks()).resolves.toEqual(anchors);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("returns no links when the section is absent", async () => {
    vi.stubGlobal("document", { querySelector: () => null });
    try {
      const page = pageOf({ evaluate: async (fn: unknown) => (fn as () => unknown)() });
      await expect(new WebNewDriver(page).codeReviewLinks()).resolves.toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("fills, submits, and waits for the cleared input after a successful attach", async () => {
    const filled: string[] = [];
    const clicked: string[] = [];
    const form = locator({ getByRole: () => locator({ click: async () => void clicked.push("Attach") }) });
    const page = pageOf({
      getByPlaceholder: () =>
        locator({
          fill: async (value: string) => void filled.push(value),
          locator: () => form,
        }),
      waitForFunction: async () => undefined,
    });
    await new WebNewDriver(page).codeReviewAttach("https://github.com/o/h/pull/2");
    expect(filled).toEqual(["https://github.com/o/h/pull/2"]);
    expect(clicked).toEqual(["Attach"]);
  });

  it("attempts an attach without waiting for the input to clear", async () => {
    const waitForFunction = vi.fn(async () => undefined);
    const page = pageOf({ waitForFunction });
    await new WebNewDriver(page).codeReviewAttemptAttach("https://bad.example/x");
    expect(waitForFunction).not.toHaveBeenCalled();
  });

  it("reads the input's current value", async () => {
    const page = pageOf({ getByPlaceholder: () => locator({ inputValue: async () => "https://kept" }) });
    await expect(new WebNewDriver(page).codeReviewInputValue()).resolves.toBe("https://kept");
  });

  it("hovers the row, detaches it, and waits for the title to leave the page", async () => {
    const events: string[] = [];
    const page = pageOf({
      locator: () =>
        locator({
          filter: () =>
            locator({
              hover: async () => void events.push("hover"),
              getByRole: () => locator({ click: async () => void events.push("click") }),
            }),
        }),
      waitForFunction: async () => void events.push("wait"),
    });
    await new WebNewDriver(page).codeReviewDetach("octocat/hello#1");
    expect(events).toEqual(["hover", "click", "wait"]);
  });
});

describe("worklog absence and toast readers", () => {
  it("finds no worklog control in the community build", async () => {
    const page = pageOf({
      locator: () =>
        locator({
          allInnerTexts: async () => ["Attachments", "Attach", "Code reviews"],
          all: async () => [],
        }),
    });
    await expect(new WebNewDriver(page).worklogCreateVisible()).resolves.toBe(false);
  });

  it("finds a worklog control when one is present", async () => {
    const page = pageOf({
      locator: () => locator({ allInnerTexts: async () => ["Worklog"], all: async () => [] }),
    });
    await expect(new WebNewDriver(page).worklogCreateVisible()).resolves.toBe(true);
  });

  it("reads the newest toast and ignores empty ones", async () => {
    const texts = ["Attachment uploaded", "  "];
    const page = pageOf({
      locator: () =>
        locator({
          count: async () => texts.length,
          first: () => locator({ innerText: async () => texts[0] ?? "" }),
        }),
    });
    await expect(new WebNewDriver(page).lastToast()).resolves.toBe("Attachment uploaded");
  });

  it("returns null when no toast is mounted", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).lastToast()).resolves.toBeNull();
  });

  it("checks the page text for a toast message", async () => {
    const page = pageOf({ locator: () => locator({ innerText: async () => "Attachment uploaded" }) });
    await expect(new WebNewDriver(page).sawToast("Attachment uploaded")).resolves.toBe(true);
    await expect(new WebNewDriver(page).sawToast("Attachment failed")).resolves.toBe(false);
  });
});
