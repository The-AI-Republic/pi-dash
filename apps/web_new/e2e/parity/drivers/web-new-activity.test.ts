// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver activity and reaction methods
// (NEWFRONT-654, ISS-194–196, ISS-203/204). A stub Page records the
// selectors the methods read, so a markup change that breaks the parity
// hooks fails here before the parity stack is ever brought up.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  first?: () => StubLocator;
  locator?: (selector: string, options?: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: RegExp | string, options?: unknown) => StubLocator;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  evaluateAll?: (fn: (nodes: never[]) => unknown) => Promise<unknown>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  allInnerTexts?: () => Promise<string[]>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  [key: string]: unknown;
}

/** One stub locator; children fall back to the same node. */
function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isEnabled: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    evaluate: async () => null,
    evaluateAll: async () => [],
    boundingBox: async () => null,
    allInnerTexts: async () => [],
    innerText: async () => "",
    textContent: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    locator: () => node,
    getByRole: () => node,
    getByText: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

interface Recorded {
  selectors: string[];
  roles: string[];
  clicks: string[];
  hovers: string[];
  keys: string[];
  evaluated: number;
}

function pageOf(
  parts: {
    locator?: (selector: string) => Locator;
    role?: (role: string, options?: unknown) => Locator;
    evaluate?: (fn: (arg?: unknown) => unknown, arg?: unknown) => unknown;
    waitForTimeout?: (ms: number) => Promise<void>;
  } = {}
): { page: Page; recorded: Recorded } {
  const recorded: Recorded = { selectors: [], roles: [], clicks: [], hovers: [], keys: [], evaluated: 0 };
  const page = {
    keyboard: {
      press: async (key: string) => {
        recorded.keys.push(key);
      },
    },
    getByRole: (role: string, options?: unknown) => {
      recorded.roles.push(role);
      return parts.role?.(role, options) ?? locator();
    },
    locator: (selector: string) => {
      recorded.selectors.push(selector);
      return parts.locator?.(selector) ?? locator();
    },
    evaluate: async (fn: (arg?: unknown) => unknown, arg?: unknown) => {
      recorded.evaluated += 1;
      return parts.evaluate === undefined ? fn(arg) : parts.evaluate(fn, arg);
    },
    waitForFunction: async () => undefined,
    waitForTimeout: parts.waitForTimeout ?? (async () => undefined),
    url: () => "https://p.test/acme/browse/WEB-1",
  } as unknown as Page;
  return { page, recorded };
}

describe("activity feed driver methods", () => {
  it("reads the timeline rows as one trimmed text per row", async () => {
    const rows = locator({
      evaluateAll: async () =>
        [" Ada created the work item.  ", "", "  A comment  "].map((text) => text.replace(/\s+/g, " ").trim()),
    });
    const { page, recorded } = pageOf({
      role: (role, options) => {
        if (role === "region" && (options as { name?: string }).name === "Activity") {
          return locator({
            locator: (selector) => {
              expect(selector).toBe('[data-feed-row="true"]');
              return rows;
            },
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).activityEntryTexts()).resolves.toEqual([
      "Ada created the work item.",
      "A comment",
    ]);
    expect(recorded.roles).toContain("region");
  });

  it("clicks the header button that is not a popover trigger to sort", async () => {
    const clicked: string[] = [];
    const sort = locator({ click: async () => void clicked.push("sort") });
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              locator: (selector) => {
                expect(selector).toBe(":scope > div");
                return locator({
                  locator: (inner) => {
                    if (inner === "button:not([aria-expanded])") return sort;
                    throw new Error(`unexpected selector ${inner}`);
                  },
                });
              },
            })
          : locator(),
    });
    await new WebNewDriver(page).activityToggleSort();
    expect(clicked).toEqual(["sort"]);
  });

  it("opens the filter popover from its trigger and reads the four toggles", async () => {
    const clicks: string[] = [];
    const filter = locator({ click: async () => void clicks.push("filter") });
    const panel = locator({
      evaluateAll: async () =>
        ["Updates", "Comments", "State", "Assignee"].map((label, index) => ({
          label,
          selected: index !== 1,
        })),
    });
    const { page, recorded } = pageOf({
      role: (role, roleOptions) => {
        if (role === "region") {
          return locator({
            locator: () => locator({ locator: () => filter }),
          });
        }
        if (role === "group" && (roleOptions as { name?: string }).name === "Activity categories") {
          // First ask (before opening) reports nothing; afterwards the panel.
          return clicks.length === 0 ? locator({ count: async () => 0 }) : locator({ locator: () => panel });
        }
        return locator();
      },
    });
    const options = await new WebNewDriver(page).activityFilterOptions();
    expect(clicks).toEqual(["filter"]);
    expect(options).toEqual([
      { label: "Updates", selected: true },
      { label: "Comments", selected: false },
      { label: "State", selected: true },
      { label: "Assignee", selected: true },
    ]);
    // The menu closes again with Escape once it has been read.
    expect(recorded.keys).toEqual(["Escape", "Escape"]);
  });

  it("toggles one category by its label and closes the popover", async () => {
    const toggled: string[] = [];
    const filter = locator();
    const { page, recorded } = pageOf({
      role: (role) => {
        if (role === "region") return locator({ locator: () => locator({ locator: () => filter }) });
        if (role === "group") {
          return locator({
            locator: () => locator(),
            getByRole: (innerRole, innerOptions) => {
              expect(innerRole).toBe("checkbox");
              expect((innerOptions as { exact?: boolean }).exact).toBe(true);
              return locator({
                click: async () => void toggled.push(String((innerOptions as { name?: string }).name)),
              });
            },
          });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).activityToggleFilter("Comments");
    expect(toggled).toEqual(["Comments"]);
    expect(recorded.keys).toContain("Escape");
  });

  it("reads the narrowed marker off the filter trigger", async () => {
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              locator: () =>
                locator({
                  locator: (selector) => {
                    if (selector === "button[aria-expanded]") {
                      return locator({
                        locator: (inner) => {
                          expect(inner).toBe('span[aria-hidden="true"]');
                          return locator({ count: async () => 1 });
                        },
                      });
                    }
                    return locator();
                  },
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).activityFilterDotVisible()).resolves.toBe(true);
  });

  it("compares the composer box with the first timeline row", async () => {
    const { page } = pageOf({
      role: (role, options) => {
        if (role === "group" && (options as { name?: string }).name === "Add comment") {
          return locator({ boundingBox: async () => ({ x: 0, y: 10, width: 1, height: 1 }) });
        }
        if (role === "region") {
          return locator({
            locator: () =>
              locator({
                first: () => locator({ boundingBox: async () => ({ x: 0, y: 50, width: 1, height: 1 }) }),
              }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).activityComposerPosition()).resolves.toBe("above");
  });

  it("reports a hidden composer and the lower position", async () => {
    const hidden = pageOf({
      role: (role, options) =>
        role === "group" && (options as { name?: string }).name === "Add comment"
          ? locator({ count: async () => 0 })
          : locator(),
    });
    await expect(new WebNewDriver(hidden.page).activityComposerPosition()).resolves.toBe("hidden");
    const below = pageOf({
      role: (role) => {
        if (role === "group") {
          return locator({ boundingBox: async () => ({ x: 0, y: 90, width: 1, height: 1 }) });
        }
        if (role === "region") {
          return locator({
            locator: () =>
              locator({ first: () => locator({ boundingBox: async () => ({ x: 0, y: 20, width: 1, height: 1 }) }) }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(below.page).activityComposerPosition()).resolves.toBe("below");
  });

  it("reports the skeleton and the stored choices", async () => {
    const stored: Record<string, string | null> = {
      activity_sort_order: JSON.stringify("desc"),
      issue_activity_filters: JSON.stringify(["COMMENT"]),
    };
    const { page } = pageOf({
      role: (role) => (role === "region" ? locator({ getByRole: () => locator({ count: async () => 3 }) }) : locator()),
      evaluate: async (fn, arg) => {
        // The driver's callbacks read `window.localStorage`; stand a
        // minimal window up for the duration of the call.
        const scope = globalThis as { window?: unknown };
        const saved = scope.window;
        scope.window = { localStorage: { getItem: (key: string) => stored[key] ?? null } };
        try {
          return fn(arg);
        } finally {
          scope.window = saved;
        }
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.activityLoadingVisible()).resolves.toBe(true);
    await expect(driver.activityStoredSort()).resolves.toBe(JSON.stringify("desc"));
    await expect(driver.activityStoredFilters()).resolves.toBe(JSON.stringify(["COMMENT"]));
  });

  it("spots the creation entry by its sentence", async () => {
    const { page, recorded } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              getByText: (text) => {
                expect(text).toBeInstanceOf(RegExp);
                expect((text as RegExp).test("Ada created the work item.")).toBe(true);
                return locator({ count: async () => 1 });
              },
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).activityHasCreationEntry()).resolves.toBe(true);
    expect(recorded.roles).toContain("region");
  });

  it("keeps polling while the feed paints, then reports the entry", async () => {
    let reads = 0;
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              getByText: () => {
                reads += 1;
                // The timeline query has not settled on the first read.
                return locator({ count: async () => (reads === 1 ? 0 : 1) });
              },
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).activityHasCreationEntry()).resolves.toBe(true);
    expect(reads).toBe(2);
  });

  it("gives up and reports no entry once the wait budget is spent", async () => {
    let polls = 0;
    vi.useFakeTimers();
    try {
      const { page } = pageOf({
        role: (role) =>
          role === "region" ? locator({ getByText: () => locator({ count: async () => 0 }) }) : locator(),
        waitForTimeout: async () => {
          polls += 1;
          vi.advanceTimersByTime(250);
        },
      });
      await expect(new WebNewDriver(page).activityHasCreationEntry()).resolves.toBe(false);
      expect(polls).toBeGreaterThan(1);
      expect(polls).toBeLessThan(1000);
    } finally {
      vi.useRealTimers();
    }
  });

  it("posts a comment by typing then clicking the submit button", async () => {
    const typed: string[] = [];
    let submitted = 0;
    let cardWaited = 0;
    const editor = locator({
      pressSequentially: async (text: string) => {
        typed.push(text);
      },
    });
    const { page } = pageOf({
      role: (role, options) => {
        if (role === "group") {
          return locator({
            locator: () => editor,
            getByRole: (inner: string, innerOptions?: unknown) => {
              expect(inner).toBe("button");
              expect((innerOptions as { name?: unknown }).name).toBe("Comment");
              return locator({
                click: async () => {
                  submitted += 1;
                },
              });
            },
          });
        }
        if (role === "article") {
          expect((options as { hasText?: unknown }).hasText).toBe("hello");
          return locator({
            waitFor: async () => {
              cardWaited += 1;
            },
          });
        }
        return locator();
      },
    });
    await new WebNewDriver(page).activityPostComment("hello");
    expect(typed).toEqual(["hello"]);
    expect(submitted).toBe(1);
    expect(cardWaited).toBe(1);
  });

  it("waits for the first feed row before reading comment texts", async () => {
    let reads = 0;
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              locator: (selector: string) => {
                expect(selector).toBe('[data-feed-row="true"]');
                reads += 1;
                return locator({ count: async () => (reads === 1 ? 0 : 2) });
              },
            })
          : locator(),
      locator: (selector: string) =>
        selector === '[id^="comment-"]' ? locator({ allInnerTexts: async () => ["a", "b"] }) : locator(),
    });
    await expect(new WebNewDriver(page).activityCommentTexts()).resolves.toEqual(["a", "b"]);
    expect(reads).toBe(2);
  });

  it("reads an empty feed as [] once the wait budget is spent", async () => {
    vi.useFakeTimers();
    try {
      const { page } = pageOf({
        role: (role) => (role === "region" ? locator({ locator: () => locator({ count: async () => 0 }) }) : locator()),
        locator: () => locator({ allInnerTexts: async () => [] }),
        waitForTimeout: async () => {
          vi.advanceTimersByTime(250);
        },
      });
      await expect(new WebNewDriver(page).activityCommentTexts()).resolves.toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("reaction driver methods", () => {
  it("picks the first emoji and returns its character and code-point key", async () => {
    const trigger = locator();
    const entry = locator({ textContent: async () => " 👍 ", click: async () => undefined });
    const { page, recorded } = pageOf({
      locator: (selector) => {
        expect(selector).toBe('button[data-slot="emoji-picker-list-emoji"]');
        return locator({ first: () => entry });
      },
    });
    const picked = await new WebNewDriver(page).activityAddCommentReaction("Some body", trigger);
    expect(recorded.selectors).toContain('button[data-slot="emoji-picker-list-emoji"]');
    expect(picked).toEqual({ emoji: "👍", code: "128077" });
    expect(recorded.keys).toContain("Escape");
  });
});

describe("chip parsing", () => {
  it("skips the picker trigger, blank text and folded labels, and parses emoji counts", async () => {
    const entries = [
      { text: "👍2", cls: "border-accent-strong", popup: "dialog" },
      { text: "👍2", cls: "border-accent-strong", popup: null },
      { text: "🎉 1", cls: "border-(--border)", popup: null },
      { text: "", cls: "", popup: null },
      { text: "Folded comment · Click to expand", cls: "", popup: null },
    ];
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({
              evaluate: async (fn) =>
                fn({
                  querySelectorAll: () =>
                    entries.map((entry) => ({
                      innerText: entry.text,
                      className: entry.cls,
                      getAttribute: () => entry.popup,
                    })),
                } as never),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).issueReactionChips()).resolves.toEqual([
      { emoji: "👍", count: 2, reacted: true },
      { emoji: "🎉", count: 1, reacted: false },
    ]);
  });

  it("shows the issue picker only while it is enabled", async () => {
    const { page } = pageOf({
      role: (role) =>
        role === "region"
          ? locator({ locator: () => locator({ first: () => locator({ isEnabled: async () => false }) }) })
          : locator(),
    });
    await expect(new WebNewDriver(page).reactionsIssuePickerReady()).resolves.toBe(false);
  });

  it("reports an archived card as carrying no reaction cluster", async () => {
    const { page } = pageOf({
      role: (role) =>
        role === "article"
          ? locator({
              locator: (selector) => {
                expect(selector).toBe('button[aria-haspopup="dialog"]');
                return locator({ count: async () => 0 });
              },
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).reactionsArchivedCommentClusterVisible("Body")).resolves.toBe(false);
  });
});
