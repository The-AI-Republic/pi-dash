// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver comment methods (NEWFRONT-651,
// ISS-197–202). A stub Page stands in for Playwright; each assertion is
// about the selector or value the method reads back.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  pressSequentially?: (value: string, options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  allInnerTexts?: () => Promise<string[]>;
  innerText?: () => Promise<string>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  first?: () => StubLocator;
  locator?: (selector: string) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    pressSequentially: async () => undefined,
    waitFor: async () => undefined,
    allInnerTexts: async () => [],
    innerText: async () => "",
    evaluate: async () => null,
    boundingBox: async () => null,
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

interface PageStub {
  page: Page;
  clicks: string[];
  presses: string[];
  typed: string[];
  escapes: number;
}

function pageOf(
  parts: {
    cards?: (text: string) => Locator;
    composer?: Locator;
    menu?: Locator;
    menuitem?: (name: string) => Locator;
    locator?: (selector: string) => Locator;
    body?: Locator;
  } = {}
): PageStub {
  const state: PageStub = {
    clicks: [],
    presses: [],
    typed: [],
    escapes: 0,
    page: undefined as unknown as Page,
  };
  const cards = parts.cards ?? (() => locator());
  state.page = {
    keyboard: {
      press: async (key: string) => {
        state.presses.push(key);
        if (key === "Escape") state.escapes += 1;
      },
    },
    getByRole: (role: string, options?: { name?: string | RegExp }) => {
      if (role === "group") return parts.composer ?? locator();
      if (role === "menuitem") {
        return parts.menuitem?.(String(options?.name)) ?? locator();
      }
      if (role === "article") return cards(String((options as { hasText?: string } | undefined)?.hasText ?? ""));
      if (role === "button") {
        return locator({
          click: async () => {
            state.clicks.push(`button:${String(options?.name)}`);
          },
        });
      }
      return locator();
    },
    locator: (selector: string) => {
      if (selector === "body") return parts.body ?? locator({ innerText: async () => "" });
      return parts.locator?.(selector) ?? locator();
    },
    context: () => ({ grantPermissions: async () => undefined }),
    waitForFunction: async () => undefined,
    url: () => "https://p.test/acme/issues",
    reload: async () => null,
    evaluate: async (fn: (arg?: unknown) => unknown) => fn(),
  } as unknown as Page;
  return state;
}

describe("web_new comment driver", () => {
  it("opens a detail by key and waits for the comment block", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Comments") {
        return locator({ waitFor: async () => void visited.push("comments") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "WEB-1");
    expect(visited).toEqual(["/acme/browse/WEB-1", "comments"]);
  });

  it("opens a detail by ids through the short link and its browse redirect", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    page.page.waitForLoadState = (async () => {
      visited.push("load");
    }) as Page["waitForLoadState"];
    page.page.waitForURL = (async (pattern: RegExp) => {
      expect(pattern.test("/acme/browse/WEB-1")).toBe(true);
      visited.push("url");
      return null;
    }) as Page["waitForURL"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Comments") {
        return locator({ waitFor: async () => void visited.push("comments") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "p-1", "i-1");
    expect(visited).toEqual(["/acme/projects/p-1/issues/i-1", "load", "url", "comments"]);
  });

  it("reloads when already on a browse view", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.url = () => "https://p.test/acme/browse/WEB-1";
    page.page.reload = (async () => {
      visited.push("reload");
      return null;
    }) as Page["reload"];
    page.page.waitForURL = (async () => {
      visited.push("url");
      return null;
    }) as Page["waitForURL"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Comments") {
        return locator({ waitFor: async () => void visited.push("comments") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "p-1", "i-1");
    expect(visited).toEqual(["reload", "url", "comments"]);
  });

  it("reads the card texts in feed order", async () => {
    const page = pageOf({
      locator: (selector) =>
        selector === '[id^="comment-"]' ? locator({ allInnerTexts: async () => ["first", "second"] }) : locator(),
    });
    await expect(new WebNewDriver(page.page).activityCommentTexts()).resolves.toEqual(["first", "second"]);
  });

  it("compares the composer top with the first card", async () => {
    const composer = locator({ boundingBox: async () => ({ x: 0, y: 10, width: 1, height: 1 }) });
    const page = pageOf({
      composer,
      locator: (selector) =>
        selector === '[id^="comment-"]'
          ? locator({ first: () => locator({ boundingBox: async () => ({ x: 0, y: 40, width: 1, height: 1 }) }) })
          : locator(),
    });
    await expect(new WebNewDriver(page.page).activityComposerIsAboveFeed()).resolves.toBe(true);
  });

  it("fails the ordering check when either box is missing", async () => {
    const page = pageOf({ composer: locator({ boundingBox: async () => null }) });
    await expect(new WebNewDriver(page.page).activityComposerIsAboveFeed()).rejects.toThrow(/no layout box/);
  });

  it("reads the trimmed composer text from the editable surface", async () => {
    const editor = locator({ innerText: async () => "  a draft  " });
    const page = pageOf({ composer: locator({ locator: () => editor }) });
    await expect(new WebNewDriver(page.page).activityComposerText()).resolves.toBe("a draft");
  });

  it("types into the composer, presses Enter and waits for the card", async () => {
    const steps: string[] = [];
    const editor = locator({
      click: async () => void steps.push("click"),
      fill: async (value: string) => void steps.push(`fill:${value}`),
      press: async (key: string) => void steps.push(`press:${key}`),
    });
    const page = pageOf({
      composer: locator({ locator: () => editor }),
      cards: (text) =>
        locator({
          waitFor: async () => {
            steps.push(`card:${text}`);
          },
        }),
    });
    await new WebNewDriver(page.page).activityPostComment("Hello there");
    expect(steps).toEqual(["click", "fill:Hello there", "press:Enter", "card:Hello there"]);
  });

  it("hovers the card, opens the menu trigger and waits for an item", async () => {
    const steps: string[] = [];
    let menuReady = false;
    const page = pageOf({
      cards: () =>
        locator({
          hover: async () => void steps.push("hover"),
          locator: (selector) => {
            expect(selector).toBe('button[aria-haspopup="menu"]');
            return locator({
              first: () =>
                locator({
                  click: async () => {
                    steps.push("trigger");
                    menuReady = true;
                  },
                }),
            });
          },
        }),
      menuitem: () =>
        locator({
          waitFor: async () => {
            expect(menuReady).toBe(true);
            steps.push("menu");
          },
        }),
    });
    await new WebNewDriver(page.page).activityOpenCommentMenu("Hello");
    expect(steps).toEqual(["hover", "trigger", "menu"]);
    expect(page.presses).toContain("Escape");
  });

  it("lists the open menu items and clicks one by name", async () => {
    const clicked: string[] = [];
    const page = pageOf({
      locator: (selector) => (selector === '[id^="comment-"]' ? locator() : locator()),
      menuitem: (name) =>
        locator({
          click: async () => {
            if (name === "Delete") clicked.push(name);
          },
        }),
    });
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "article") return locator();
      if (role === "menuitem") {
        return options?.name === undefined
          ? locator({ allInnerTexts: async () => ["Edit", "Copy link", "Fold comment", "Delete"] })
          : locator({ click: async () => void clicked.push(String(options.name)) });
      }
      return locator();
    }) as Page["getByRole"];
    const driver = new WebNewDriver(page.page);
    await expect(driver.activityMenuItems()).resolves.toEqual(["Edit", "Copy link", "Fold comment", "Delete"]);
    await driver.activityClickMenuItem("Delete");
    expect(clicked).toEqual(["Delete"]);
  });

  it("replaces the body and saves through the check affordance", async () => {
    const steps: string[] = [];
    const editor = locator({
      click: async () => void steps.push("editor click"),
      press: async (key: string) => void steps.push(`press:${key}`),
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
      locator: (selector) => {
        expect(selector).toBe("xpath=ancestor::form[1]");
        return locator({
          getByRole: (role, options) =>
            locator({
              first: () =>
                locator({
                  click: async () => void steps.push(`save:${role}:${String((options as { name?: string }).name)}`),
                }),
            }),
        });
      },
    });
    const page = pageOf({
      locator: (selector) => (selector === '[id^="comment-"] [contenteditable="true"]' ? editor : locator()),
      cards: (text) =>
        locator({
          getByText: (fragment) =>
            locator({
              first: () =>
                locator({
                  waitFor: async () => void steps.push(`edited:${text}:${fragment}`),
                }),
            }),
        }),
    });
    await new WebNewDriver(page.page).activityEditComment("old", "new words");
    expect(steps).toContain("editor click");
    expect(steps).toContain("press:ControlOrMeta+a");
    expect(steps).toContain("type:new words");
    expect(steps).toContain("save:button:Save comment");
    expect(steps.some((step) => step.startsWith("edited:new words:"))).toBe(true);
  });

  it("looks for the toast text in the page body", async () => {
    const page = pageOf({ body: locator({ innerText: async () => "Comment created successfully" }) });
    const driver = new WebNewDriver(page.page);
    await expect(driver.sawToast("Comment created")).resolves.toBe(true);
    await expect(driver.sawToast("Comment removed")).resolves.toBe(false);
  });

  it("cancels the inline editor and waits for it to leave", async () => {
    const steps: string[] = [];
    const page = pageOf({
      cards: () =>
        locator({
          getByRole: (role, options) =>
            locator({
              first: () =>
                locator({
                  click: async () => void steps.push(`${role}:${String((options as { name?: string }).name)}`),
                }),
            }),
          locator: () =>
            locator({
              first: () =>
                locator({
                  waitFor: async (options) =>
                    void steps.push(`detached:${String((options as { state?: string }).state)}`),
                }),
            }),
        }),
    });
    await new WebNewDriver(page.page).activityCancelEdit("Hello");
    expect(steps).toEqual(["button:Cancel editing", "detached:detached"]);
  });

  it("reads the highlight off the card wrapper", async () => {
    const page = pageOf({
      cards: () =>
        locator({
          evaluate: async (fn: (element: never) => unknown) =>
            fn({
              parentElement: { className: "rounded p-2 bg-(--accent-subtle) ring-1 ring-(--accent)" },
            } as never),
        }),
    });
    await expect(new WebNewDriver(page.page).activityCommentHighlighted("Hello")).resolves.toBe(true);
  });

  it("reports a missing highlight as false instead of throwing", async () => {
    const page = pageOf({ cards: () => locator({ evaluate: async () => Promise.reject(new Error("gone")) }) });
    await expect(new WebNewDriver(page.page).activityCommentHighlighted("Hello")).resolves.toBe(false);
  });

  it("reports a folded body as hidden and matches a visible one", async () => {
    const hidden = pageOf({ cards: () => locator({ waitFor: async () => Promise.reject(new Error("gone")) }) });
    await expect(new WebNewDriver(hidden.page).activityCommentBodyVisible("Hello")).resolves.toBe(false);
    const visible = pageOf({ cards: () => locator({ innerText: async () => "Ada Hello world" }) });
    await expect(new WebNewDriver(visible.page).activityCommentBodyVisible("Hello")).resolves.toBe(true);
  });

  it("clicks the expand toggle of a folded card", async () => {
    const page = pageOf();
    page.page.getByRole = ((role: string, options?: { name?: RegExp }) => {
      if (role === "button" && options?.name instanceof RegExp) {
        expect(options.name.test("Folded comment · Click to expand")).toBe(true);
        return locator({ first: () => locator({ click: async () => void page.clicks.push("expand") }) });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).activityExpandFoldedComment("Hello");
    expect(page.clicks).toEqual(["expand"]);
  });

  it("copies the comment link through the card menu", async () => {
    const page = pageOf({
      cards: () => locator(),
      menuitem: () => locator(),
    });
    page.page.evaluate = (async (fn: (arg?: unknown) => unknown) => fn()) as Page["evaluate"];
    (
      page.page as unknown as { context: () => { grantPermissions: (permissions: string[]) => Promise<void> } }
    ).context = () => ({ grantPermissions: async () => undefined });
    const readText = (): Promise<string> => Promise.resolve("https://p.test/acme/browse/WEB-1#comment-1");
    Object.defineProperty(globalThis, "navigator", {
      configurable: true,
      value: { clipboard: { readText } },
    });
    const driver = new WebNewDriver(page.page);
    await expect(driver.activityCopyCommentLink("Hello")).resolves.toContain("#comment-1");
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: globalThis.navigator });
  });
});
