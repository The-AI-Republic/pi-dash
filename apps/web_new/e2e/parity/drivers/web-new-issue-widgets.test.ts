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

describe("widget titles", () => {
  it("returns the section titles the page reports", async () => {
    const page = pageOf({ evaluate: async () => ["Sub-work items", "Attachments"] });
    await expect(new WebNewDriver(page).widgetTitles()).resolves.toEqual(["Sub-work items", "Attachments"]);
  });

  it("returns no titles when the widget root is absent", async () => {
    const page = pageOf({ evaluate: async () => [] });
    await expect(new WebNewDriver(page).widgetTitles()).resolves.toEqual([]);
  });
});

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
              expanded = "true";
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

  it("re-clicks when the first header click never opens the section", async () => {
    let clicks = 0;
    let expanded: string | null = "false";
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="sub-issues-collapsible"]') {
          return locator({
            getAttribute: async (name: string) => (name === "aria-expanded" ? expanded : null),
            click: async () => {
              clicks += 1;
              // The first click lands on a remounting node and is lost;
              // the second takes.
              if (clicks >= 2) expanded = "true";
            },
          });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.openWidgetSection("Sub-work items");
    expect(clicks).toBe(2);
    await expect(driver.widgetExpanded("Sub-work items")).resolves.toBe(true);
  });

  it("stops after three lost clicks instead of toggling forever", async () => {
    let clicks = 0;
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="sub-issues-collapsible"]') {
          return locator({
            getAttribute: async (name: string) => (name === "aria-expanded" ? "false" : null),
            click: async () => {
              clicks += 1;
            },
          });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).openWidgetSection("Sub-work items");
    expect(clicks).toBe(3);
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

describe("cloud worklog drivers (NEWFRONT-672, ISS-206 cloud leg)", () => {
  it("logs an entry through the header control and waits for the dialog to close", async () => {
    const events: string[] = [];
    const field = (name: string) =>
      locator({
        fill: async (value: string) => void events.push(`fill:${name}=${value}`),
      });
    const dialog = locator({
      getByLabel: ((label: string) => field(label)) as never,
      getByRole: (() => locator({ click: async () => void events.push("save") })) as never,
      waitFor: (async () => void events.push("closed")) as never,
    });
    const page = pageOf({
      locator: () =>
        locator({
          getByRole: (() => locator({ click: async () => void events.push("open") })) as never,
        }),
      getByRole: (() => dialog) as never,
    });
    await new WebNewDriver(page).worklogLogEntry(90, "2026-10-20", "Deep work");
    expect(events).toEqual([
      "open",
      "fill:Minutes=90",
      "fill:Date=2026-10-20",
      "fill:Note=Deep work",
      "save",
      "closed",
    ]);
  });

  it("skips the note field when no note is given", async () => {
    const events: string[] = [];
    const dialog = locator({
      getByLabel: ((label: string) => locator({ fill: async () => void events.push(`fill:${label}`) })) as never,
      getByRole: (() => locator()) as never,
    });
    const page = pageOf({ getByRole: (() => dialog) as never });
    await new WebNewDriver(page).worklogLogEntry(30, "2026-10-20", "");
    expect(events).toEqual(["fill:Minutes", "fill:Date"]);
  });

  it("reads the sidebar total and nulls a missing or blank one", async () => {
    const page = pageOf({
      locator: () => locator({ count: async () => 1, first: () => locator({ innerText: async () => "  2h  " }) }),
    });
    await expect(new WebNewDriver(page).worklogSidebarTotal()).resolves.toBe("2h");
    const missing = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing).worklogSidebarTotal()).resolves.toBeNull();
    const blank = pageOf({
      locator: () => locator({ count: async () => 1, first: () => locator({ innerText: async () => "  " }) }),
    });
    await expect(new WebNewDriver(blank).worklogSidebarTotal()).resolves.toBeNull();
  });

  it("reads the sidebar entry rows in order", async () => {
    const page = pageOf({
      locator: () => locator({ allInnerTexts: async () => ["1h 30m · Oct 20, 2026", "30m · Oct 19, 2026"] }),
    });
    await expect(new WebNewDriver(page).worklogSidebarEntryTexts()).resolves.toEqual([
      "1h 30m · Oct 20, 2026",
      "30m · Oct 19, 2026",
    ]);
  });
});

describe("sub-issue add flows", () => {
  const ACTION = '[data-widget-action="Add sub-work item"]';

  it("counts the header row controls, zero when the header is absent", async () => {
    const row = locator({ locator: () => locator({ count: async () => 3 }) });
    const header = locator({ locator: () => row });
    const page = pageOf({
      locator: (selector: string) => (selector === '[data-testid="sub-issues-collapsible"]' ? header : locator()),
    });
    await expect(new WebNewDriver(page).widgetHeaderControlCount("Sub-work items")).resolves.toBe(3);
    const bare = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(bare).widgetHeaderControlCount("Sub-work items")).resolves.toBe(0);
  });

  function addMenuPage(picked: string[]): Page {
    return pageOf({
      locator: (selector: string) =>
        selector === ACTION ? locator({ click: async () => void picked.push("action") }) : locator(),
      getByRole: (role: string, options?: unknown) => {
        if (role === "menuitem") {
          const want = (options as { name?: string } | undefined)?.name ?? "";
          return locator({ click: async () => void picked.push(want) });
        }
        return locator();
      },
    });
  }

  it("opens the create modal through the add menu", async () => {
    const picked: string[] = [];
    await new WebNewDriver(addMenuPage(picked)).openSubIssueCreateModal();
    expect(picked).toEqual(["action", "Create new"]);
  });

  it("reads the preset parent tag, null when absent or blank", async () => {
    const tag = (text: string, present: boolean) =>
      pageOf({
        getByRole: () =>
          locator({
            getByTestId: (id: string) =>
              id === "parent-tag"
                ? locator({ count: async () => (present ? 1 : 0), innerText: async () => text })
                : locator(),
          }),
      });
    await expect(new WebNewDriver(tag("  ABC-12\nParent Name ", true)).createModalParentName()).resolves.toBe(
      "ABC-12 Parent Name"
    );
    await expect(new WebNewDriver(tag("", false)).createModalParentName()).resolves.toBeNull();
    await expect(new WebNewDriver(tag("   ", true)).createModalParentName()).resolves.toBeNull();
  });

  it("reads the project lock from the chip disabled state", async () => {
    const chipPage = (present: boolean, disabled: boolean) =>
      pageOf({
        getByRole: () =>
          locator({
            locator: () =>
              locator({
                getByRole: () =>
                  locator({
                    count: async () => (present ? 1 : 0),
                    isDisabled: async () => disabled,
                  }),
              }),
          }),
      });
    await expect(new WebNewDriver(chipPage(true, true)).createModalProjectLocked()).resolves.toBe(true);
    await expect(new WebNewDriver(chipPage(true, false)).createModalProjectLocked()).resolves.toBe(false);
    await expect(new WebNewDriver(chipPage(false, false)).createModalProjectLocked()).resolves.toBe(false);
  });

  it("submits the modal with the given name and waits for it to close", async () => {
    const calls: string[] = [];
    const field = locator({
      click: async () => void calls.push("focus"),
      inputValue: async () => "Child",
    });
    const dialog = locator({
      getByPlaceholder: (text: string) => (text === "Title" ? field : locator()),
      locator: (selector: string) =>
        selector === 'button[type="submit"]' ? locator({ click: async () => void calls.push("submit") }) : locator(),
      waitFor: async (options?: unknown) => void calls.push(`wait:${JSON.stringify(options)}`),
    });
    const page = pageOf({
      getByRole: () => dialog,
      keyboard: {
        press: async (key: string) => void calls.push(`press:${key}`),
        type: async (text: string) => void calls.push(`type:${text}`),
      },
    });
    await new WebNewDriver(page).createModalSubmit("Child");
    expect(calls).toContain("type:Child");
    expect(calls).toContain("submit");
    expect(calls).toContain(`wait:${JSON.stringify({ state: "detached", timeout: 60_000 })}`);
  });

  it("adds an existing issue through search, pick and submit", async () => {
    const picked: string[] = [];
    const dialog = locator({
      getByLabel: (label: string) =>
        label === "Search work items"
          ? locator({ fill: async (value: string) => void picked.push(`search:${value}`) })
          : locator(),
      getByRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name ?? "";
        if (role === "option") return locator({ click: async () => void picked.push(`pick:${want}`) });
        if (role === "button" && want === "Add selected work items") {
          return locator({ click: async () => void picked.push("submit") });
        }
        return locator();
      },
    });
    const rows = locator({
      count: async () => 1,
      nth: () => locator({ innerText: async () => "ABC-1 Existing child" }),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === ACTION) return locator({ click: async () => void picked.push("action") });
        if (selector === '[data-testid="sub-issues-section"]') {
          return locator({ locator: () => rows });
        }
        return locator();
      },
      getByRole: (role: string, options?: unknown) => {
        if (role === "dialog") return dialog;
        if (role === "menuitem") {
          const want = (options as { name?: string } | undefined)?.name ?? "";
          return locator({ click: async () => void picked.push(want) });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).addExistingSubIssue("Existing", "Existing child");
    expect(picked).toContain("action");
    expect(picked).toContain("Add existing");
    expect(picked).toContain("search:Existing");
    expect(picked).toContain("pick:Existing child");
    expect(picked).toContain("submit");
  });
});

describe("relations, links and run-ai widgets", () => {
  function sectionPage(sectionId: string, rows: (selector: string) => StubLocator): Page {
    return pageOf({
      getByTestId: (id: string) =>
        id === sectionId ? locator({ locator: (selector: string) => rows(selector) }) : locator(),
    });
  }

  it("reads relation and link rows from their prefixed testids", async () => {
    const relations = sectionPage("widget-relations", (selector: string) =>
      selector === '[data-testid^="relation-row-"]'
        ? locator({ count: async () => 1, nth: () => locator({ innerText: async () => "  P-1  Related " }) })
        : locator({ count: async () => 0 })
    );
    await expect(new WebNewDriver(relations).widgetRowNames("Relations")).resolves.toEqual(["P-1 Related"]);
    const links = sectionPage("widget-links", (selector: string) =>
      selector === '[data-testid^="link-row-"]'
        ? locator({ count: async () => 1, nth: () => locator({ innerText: async () => "https://a.example " }) })
        : locator({ count: async () => 0 })
    );
    await expect(new WebNewDriver(links).widgetRowNames("Links")).resolves.toEqual(["https://a.example"]);
  });

  it("reads relation group titles, and nothing elsewhere", async () => {
    const groups = locator({
      count: async () => 2,
      nth: (index: number) =>
        locator({
          getByRole: () => locator({ innerText: async () => (index === 0 ? "Blocked by 1" : "Relates to 1") }),
        }),
    });
    const page = sectionPage("widget-relations", (selector: string) =>
      selector === '[data-testid^="relation-group-"]' ? groups : locator({ count: async () => 0 })
    );
    await expect(new WebNewDriver(page).widgetGroupNames("Relations")).resolves.toEqual([
      "Blocked by 1",
      "Relates to 1",
    ]);
    await expect(new WebNewDriver(page).widgetGroupNames("Links")).resolves.toEqual([]);
  });

  it("reads the add menu off the widget action button", async () => {
    const calls: string[] = [];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-widget-action="Add relation"]'
          ? locator({ click: async () => void calls.push("action") })
          : locator({ allTextContents: async () => [" Relates to ", "", "Blocking"] }),
    });
    await expect(new WebNewDriver(page).widgetAddMenuNames("Relations")).resolves.toEqual(["Relates to", "Blocking"]);
    expect(calls).toEqual(["action"]);
    await expect(new WebNewDriver(page).widgetAddMenuNames("Code reviews")).rejects.toThrow("no add menu");
  });

  it("adds a relation through type pick, search, option and submit", async () => {
    const picked: string[] = [];
    const rows = locator({
      count: async () => 1,
      nth: () => locator({ innerText: async () => "P-2 Other" }),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-widget-action="Add relation"]') {
          return locator({ click: async () => void picked.push("action") });
        }
        return locator();
      },
      getByTestId: (id: string) => (id === "widget-relations" ? locator({ locator: () => rows }) : locator()),
      getByRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name ?? "";
        if (role === "menuitem") return locator({ click: async () => void picked.push("menu") });
        if (role === "listbox") {
          return locator({
            getByRole: () => locator({ click: async () => void picked.push(`pick:${want}`) }),
          });
        }
        if (role === "button" && want === "Add selected work items") {
          return locator({ click: async () => void picked.push("submit") });
        }
        return locator();
      },
      getByLabel: () => locator({ fill: async (value: string) => void picked.push(`search:${value}`) }),
    });
    await new WebNewDriver(page).addRelationViaModal("Blocked", "Other", "Other");
    expect(picked).toContain("action");
    expect(picked).toContain("menu");
    expect(picked).toContain("search:Other");
    expect(picked).toContain("submit");
  });

  it("adds a link with an optional title and waits for the close", async () => {
    const calls: string[] = [];
    const dialog = locator({
      getByLabel: (label: string) => locator({ fill: async (value: string) => void calls.push(`${label}:${value}`) }),
      getByRole: () => locator({ click: async () => void calls.push("submit") }),
      waitFor: async () => void calls.push("detached"),
    });
    const page = pageOf({
      locator: () => locator({ click: async () => void calls.push("action") }),
      getByRole: (role: string) => (role === "dialog" ? dialog : locator()),
    });
    await new WebNewDriver(page).addLinkModal("https://a.example", "A");
    expect(calls).toContain("URL:https://a.example");
    expect(calls).toContain("Display title (optional):A");
    expect(calls).toContain("submit");
    calls.length = 0;
    await new WebNewDriver(page).addLinkModal("https://b.example");
    expect(calls).not.toContain("Display title (optional):B");
    expect(calls).toContain("URL:https://b.example");
  });

  it("copies, edits and reads back link rows", async () => {
    const calls: string[] = [];
    const rows = (name: string) =>
      locator({
        filter: () =>
          locator({
            first: () =>
              locator({
                getByRole: () => locator({ click: async () => void calls.push(name) }),
                locator: (selector: string) =>
                  selector === "a[href]"
                    ? locator({
                        getAttribute: async (attr: string) =>
                          attr === "href" ? "https://a.example" : attr === "target" ? "_blank" : null,
                      })
                    : locator({ count: async () => 0 }),
              }),
          }),
      });
    const page = pageOf({
      getByTestId: (id: string) => (id === "widget-links" ? locator({ locator: () => rows("row") }) : locator()),
      getByRole: (role: string) => {
        if (role === "menuitem") return locator({ click: async () => void calls.push("edit-entry") });
        if (role === "dialog") {
          return locator({
            getByLabel: () => locator({ fill: async (value: string) => void calls.push(`title:${value}`) }),
            getByRole: () => locator({ click: async () => void calls.push("save") }),
          });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).clickLinkCopy("A");
    await new WebNewDriver(page).editLinkTitle("A", "B");
    expect(calls).toContain("row");
    expect(calls).toContain("edit-entry");
    expect(calls).toContain("title:B");
    expect(calls).toContain("save");
    await expect(new WebNewDriver(page).linkRowTarget("A")).resolves.toEqual({
      href: "https://a.example",
      target: "_blank",
    });
    const bare = pageOf({
      getByTestId: () => locator({ locator: () => locator({ filter: () => locator({ count: async () => 0 }) }) }),
    });
    await expect(new WebNewDriver(bare).linkRowTarget("A")).resolves.toBeNull();
  });

  it("clicks widget actions and drives the run composer", async () => {
    const calls: string[] = [];
    const editor = locator({
      click: async () => void calls.push("focus"),
      pressSequentially: async (text: string) => void calls.push(`type:${text}`),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-widget-action="Manually Run AI"]') {
          return locator({ click: async () => void calls.push("run-action") });
        }
        if (selector === '[contenteditable="true"]') return editor;
        return locator();
      },
      getByTestId: (id: string) => (id === "issue-run-ai" ? locator({ count: async () => 0 }) : locator()),
      getByRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name ?? "";
        if (role === "group") return locator({ locator: () => editor });
        if (role === "button" && want === "Comment & Run") {
          return locator({
            click: async () => void calls.push("run"),
            isDisabled: async () => calls.includes("typed"),
          });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.clickWidgetAction("Manually Run AI");
    await expect(driver.commentAndRunDisabled()).resolves.toBe(false);
    await driver.typeComment("hello");
    calls.push("typed");
    await driver.clickCommentAndRun();
    expect(calls).toContain("run-action");
    expect(calls).toContain("type:hello");
    expect(calls).toContain("run");
  });

  it("prefers the Run AI dispatch button when the slot renders it", async () => {
    const calls: string[] = [];
    const page = pageOf({
      locator: () => locator({ click: async () => void calls.push("action") }),
      getByTestId: (id: string) =>
        id === "issue-run-ai" ? locator({ click: async () => void calls.push("dispatch") }) : locator(),
    });
    await new WebNewDriver(page).clickWidgetAction("Manually Run AI");
    expect(calls).toEqual(["dispatch"]);
  });
});
