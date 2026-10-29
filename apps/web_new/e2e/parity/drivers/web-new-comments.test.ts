// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver comment methods (NEWFRONT-651,
// ISS-197–202). A stub Page stands in for Playwright; each assertion is
// about the selector or value the method reads back.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import type { RulesCommentMenuOption } from "./parity-driver";
import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  pressSequentially?: (value: string, options?: unknown) => Promise<void>;
  focus?: (options?: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  setInputFiles?: (files: string | readonly string[], options?: unknown) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  allInnerTexts?: () => Promise<string[]>;
  allTextContents?: () => Promise<string[]>;
  innerText?: () => Promise<string>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  all?: () => Promise<StubLocator[]>;
  locator?: (selector: string) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  filter?: (options?: unknown) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isEnabled: async () => true,
    isDisabled: async () => false,
    click: async () => undefined,
    hover: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    pressSequentially: async () => undefined,
    focus: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    setInputFiles: async () => undefined,
    getAttribute: async () => null,
    waitFor: async () => undefined,
    allInnerTexts: async () => [],
    allTextContents: async () => [],
    innerText: async () => "",
    evaluate: async () => null,
    boundingBox: async () => null,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    all: async () => [node],
    locator: () => node,
    getByRole: () => node,
    getByText: () => node,
    filter: () => node,
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
    list?: Locator;
    tooltip?: Locator;
    label?: (name: string) => Locator;
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
      if (role === "list") return parts.list ?? locator();
      if (role === "tooltip") return parts.tooltip ?? locator();
      if (role === "region") return locator({ locator: (selector: string) => parts.locator?.(selector) ?? locator() });
      if (role === "menuitem") {
        return parts.menuitem?.(String(options?.name)) ?? locator();
      }
      if (role === "article") {
        return locator({
          filter: (filterOptions?: { hasText?: string }) => cards(String(filterOptions?.hasText ?? "")),
        });
      }
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
    getByLabel: (name: string) => parts.label?.(name) ?? locator(),
    context: () => ({ grantPermissions: async () => undefined }),
    waitForFunction: async () => undefined,
    waitForTimeout: async () => undefined,
    route: async () => undefined,
    unroute: async () => undefined,
    mouse: { move: async () => undefined, click: async () => undefined },
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
      if (role === "heading" && options?.name === "Activity") {
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
      if (role === "heading" && options?.name === "Activity") {
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
      if (role === "heading" && options?.name === "Activity") {
        return locator({ waitFor: async () => void visited.push("comments") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "p-1", "i-1");
    expect(visited).toEqual(["reload", "url", "comments"]);
  });

  it("retries the short-link redirect once through a reload on a stall", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    page.page.waitForLoadState = (async () => {
      visited.push("load");
    }) as Page["waitForLoadState"];
    let urls = 0;
    page.page.waitForURL = (async () => {
      urls += 1;
      visited.push(`url${urls}`);
      if (urls === 1) throw new Error("stall");
      return null;
    }) as Page["waitForURL"];
    page.page.getByText = ((text: string) => {
      expect(text).toBe("Back to the project issues");
      return locator({ count: async () => 0 });
    }) as unknown as Page["getByText"];
    page.page.reload = (async () => {
      visited.push("reload");
      return null;
    }) as Page["reload"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Activity") {
        return locator({ waitFor: async () => void visited.push("comments") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "p-1", "i-1");
    expect(visited).toEqual(["/acme/projects/p-1/issues/i-1", "load", "url1", "reload", "url2", "comments"]);
  });

  it("throws without retrying when the missing state rendered", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    page.page.waitForLoadState = (async () => {
      visited.push("load");
    }) as Page["waitForLoadState"];
    page.page.waitForURL = (async () => {
      visited.push("url");
      throw new Error("stall");
    }) as Page["waitForURL"];
    page.page.getByText = (() => locator({ count: async () => 1 })) as unknown as Page["getByText"];
    page.page.reload = (async () => {
      visited.push("reload");
      return null;
    }) as Page["reload"];
    await expect(new WebNewDriver(page.page).openIssueDetail("acme", "p-1", "i-1")).rejects.toThrow(
      "never reached the browse view"
    );
    expect(visited).not.toContain("reload");
  });

  it("retries the browse settle once through a reload on a stall", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    let settles = 0;
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Activity") {
        return locator({
          waitFor: async () => {
            settles += 1;
            visited.push(`settle${settles}`);
            if (settles === 1) throw new Error("stall");
          },
        });
      }
      return locator();
    }) as Page["getByRole"];
    page.page.getByText = (() => locator({ count: async () => 0 })) as unknown as Page["getByText"];
    page.page.reload = (async () => {
      visited.push("reload");
      return null;
    }) as Page["reload"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "WEB-1");
    expect(visited).toEqual(["/acme/browse/WEB-1", "settle1", "reload", "settle2"]);
  });

  it("returns without retrying when the browse missing state rendered", async () => {
    const visited: string[] = [];
    const page = pageOf();
    page.page.goto = (async (url: string) => {
      visited.push(url);
      return null;
    }) as Page["goto"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading" && options?.name === "Activity") {
        return locator({
          waitFor: async () => {
            throw new Error("stall");
          },
        });
      }
      return locator();
    }) as Page["getByRole"];
    page.page.getByText = ((text: string | RegExp) => {
      expect(String(text)).toContain("No work item matches this address");
      return locator({ count: async () => 1 });
    }) as unknown as Page["getByText"];
    page.page.reload = (async () => {
      visited.push("reload");
      return null;
    }) as Page["reload"];
    await new WebNewDriver(page.page).openIssueDetail("acme", "WEB-1");
    expect(visited).toEqual(["/acme/browse/WEB-1"]);
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
        selector === '[data-feed-row="true"]'
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

  it("types into the composer, clicks Comment and waits for the card", async () => {
    const steps: string[] = [];
    const editor = locator({
      click: async () => void steps.push("click"),
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
    });
    const page = pageOf({
      composer: locator({
        locator: () => editor,
        getByRole: (role: string, options?: unknown) => {
          expect(role).toBe("button");
          expect((options as { name?: unknown }).name).toBe("Comment");
          return locator({ click: async () => void steps.push("submit") });
        },
      }),
      cards: (text) =>
        locator({
          waitFor: async () => {
            steps.push(`card:${text}`);
          },
        }),
    });
    await new WebNewDriver(page.page).activityPostComment("Hello there");
    expect(steps).toEqual(["click", "type:Hello there", "submit", "card:Hello there"]);
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

describe("web_new comments gate driver (NEWFRONT-818)", () => {
  it("opens mention, rule, activity and composer details through the shared open", async () => {
    const page = pageOf();
    const driver = new WebNewDriver(page.page);
    const calls: string[] = [];
    driver.openIssueDetail = (async (workspace: string, ref: string, id?: string) => {
      calls.push(`${workspace}/${ref}/${id ?? "-"}`);
    }) as WebNewDriver["openIssueDetail"];
    await driver.mentionsOpenIssueDetail("acme", "p-1", "i-1");
    await driver.rulesOpenIssueDetail("acme", "p-1", "i-1");
    await driver.activityOpenIssueDetail("acme", "p-1", "i-1");
    await driver.composerOpenIssue("acme", "WEB-1");
    expect(calls).toEqual(["acme/p-1/i-1", "acme/p-1/i-1", "acme/p-1/i-1", "acme/WEB-1/-"]);
  });

  it("signs activity scenarios in through the shared rules sign-in", async () => {
    const page = pageOf();
    const driver = new WebNewDriver(page.page);
    const calls: string[] = [];
    driver.rulesEnsureSignedIn = (async (email: string, password: string, slug: string) => {
      calls.push(`${email}:${password}:${slug}`);
    }) as WebNewDriver["rulesEnsureSignedIn"];
    await driver.activitySignIn("a@b.c", "pw", "acme");
    expect(calls).toEqual(["a@b.c:pw:acme"]);
  });

  it("signs mentions in on the first try when the stack answers", async () => {
    const page = pageOf();
    const driver = new WebNewDriver(page.page);
    const steps: string[] = [];
    driver.openEntry = (async () => void steps.push("entry")) as WebNewDriver["openEntry"];
    driver.signInWithPassword = (async () => void steps.push("signin")) as WebNewDriver["signInWithPassword"];
    await driver.mentionsEnsureSignedIn("a@b.c", "pw");
    expect(steps).toEqual(["entry", "signin"]);
  });

  it("takes a late landing instead of a second sign-in attempt", async () => {
    const page = pageOf();
    page.page.url = () => "https://p.test/acme";
    const driver = new WebNewDriver(page.page);
    let entries = 0;
    driver.openEntry = (async () => void (entries += 1)) as WebNewDriver["openEntry"];
    driver.signInWithPassword = (async () => {
      throw new Error("flaky");
    }) as WebNewDriver["signInWithPassword"];
    await driver.mentionsEnsureSignedIn("a@b.c", "pw");
    expect(entries).toBe(1);
  });

  it("throws after three refusals without a landing", async () => {
    const page = pageOf();
    page.page.url = () => "https://p.test/";
    const driver = new WebNewDriver(page.page);
    let entries = 0;
    driver.openEntry = (async () => void (entries += 1)) as WebNewDriver["openEntry"];
    driver.signInWithPassword = (async () => {
      throw new Error("nope 500\nsecond line");
    }) as WebNewDriver["signInWithPassword"];
    await expect(driver.mentionsEnsureSignedIn("a@b.c", "pw")).rejects.toThrow(
      "did not land after 3 tries: Error: nope 500"
    );
    expect(entries).toBe(3);
  });

  it("types the trigger and reads the suggestion names in order", async () => {
    const typed: string[] = [];
    const editor = locator({
      pressSequentially: async (value: string) => void typed.push(value),
    });
    const names = ["  Parity Mention  ", "", "Parity Oracle"];
    const items = locator({
      count: async () => names.length,
      nth: (index: number) =>
        locator({
          locator: (selector: string) => {
            expect(selector).toBe("span");
            return locator({ last: () => locator({ innerText: async () => names[index] as string }) });
          },
        }),
    });
    const page = pageOf();
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "group") {
        expect(options?.name).toBe("Add comment");
        return locator({ locator: () => editor });
      }
      if (role === "list") {
        expect(options?.name).toBe("Mention suggestions");
        return locator({
          getByRole: (inner: string) => {
            expect(inner).toBe("button");
            return items;
          },
        });
      }
      return locator();
    }) as Page["getByRole"];
    await expect(new WebNewDriver(page.page).mentionsSuggestionsFor("Pari")).resolves.toEqual([
      "Parity Mention",
      "Parity Oracle",
    ]);
    expect(typed).toEqual(["@Pari"]);
    expect(page.presses).toEqual(["Escape", "ControlOrMeta+a", "Backspace"]);
  });

  it("tolerates a failed pre-dismiss when suggesting", async () => {
    const editor = locator({ pressSequentially: async () => undefined });
    const page = pageOf({
      composer: locator({ locator: () => editor }),
      list: locator({
        getByRole: () =>
          locator({
            count: async () => 1,
            nth: () =>
              locator({
                locator: () => locator({ last: () => locator({ innerText: async () => "Parity Mention" }) }),
              }),
          }),
      }),
    });
    let presses = 0;
    page.page.keyboard.press = (async () => {
      presses += 1;
      if (presses === 1) throw new Error("no focus");
    }) as Page["keyboard"]["press"];
    await expect(new WebNewDriver(page.page).mentionsSuggestionsFor("Pari")).resolves.toEqual(["Parity Mention"]);
    expect(presses).toBe(3);
  });

  it("requires an avatar on every suggestion row", async () => {
    const withAvatar = locator({ getByRole: () => locator({ count: async () => 1 }) });
    const withoutAvatar = locator({ getByRole: () => locator({ count: async () => 0 }) });
    const full = pageOf({
      list: locator({ getByRole: () => locator({ count: async () => 2, all: async () => [withAvatar, withAvatar] }) }),
    });
    await expect(new WebNewDriver(full.page).mentionsSuggestionsHaveAvatars()).resolves.toBe(true);
    const partial = pageOf({
      list: locator({
        getByRole: () => locator({ count: async () => 2, all: async () => [withAvatar, withoutAvatar] }),
      }),
    });
    await expect(new WebNewDriver(partial.page).mentionsSuggestionsHaveAvatars()).resolves.toBe(false);
    const empty = pageOf({
      list: locator({ getByRole: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(empty.page).mentionsSuggestionsHaveAvatars()).resolves.toBe(false);
  });

  it("reads section rows and skips people rows and unreadable rows", async () => {
    const header = locator({
      getByRole: () => locator({ count: async () => 0 }),
      innerText: async () => "  Users  ",
    });
    const person = locator({ getByRole: () => locator({ count: async () => 1 }) });
    const blank = locator({
      getByRole: () => locator({ count: async () => 0 }),
      innerText: async () => "   ",
    });
    const gone = locator({
      getByRole: () => locator({ count: async () => 0 }),
      innerText: async () => Promise.reject(new Error("detached")),
    });
    const rows = [header, person, blank, gone];
    const page = pageOf({
      list: locator({
        locator: (selector: string) => {
          expect(selector).toBe(":scope > li");
          return locator({ count: async () => rows.length, nth: (index: number) => rows[index] as Locator });
        },
      }),
    });
    await expect(new WebNewDriver(page.page).mentionsSuggestionSections()).resolves.toEqual(["Users"]);
  });

  it("posts a mention comment through the trigger, pick, body and submit", async () => {
    const steps: string[] = [];
    const editor = locator({
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
    });
    const page = pageOf();
    page.page.getByRole = ((role: string, _options?: { name?: string }) => {
      if (role === "group") {
        return locator({
          locator: () => editor,
          getByRole: (inner: string, innerOptions?: { name?: string }) => {
            expect(inner).toBe("button");
            expect(innerOptions?.name).toBe("Comment");
            return locator({ first: () => locator({ click: async () => void steps.push("submit") }) });
          },
        });
      }
      if (role === "list") return locator({ getByRole: () => locator() });
      return locator();
    }) as Page["getByRole"];
    page.page.getByText = ((text: string) => {
      expect(text).toBe("hello there");
      return locator({ first: () => locator({ waitFor: async () => void steps.push("card") }) });
    }) as unknown as Page["getByText"];
    await new WebNewDriver(page.page).mentionsPostComment("Parity Mention", "hello there");
    expect(steps).toEqual(["type:@Parity Mention", "type: hello there", "submit", "card"]);
    expect(page.presses).toEqual(["Escape", "Enter"]);
  });

  it("tolerates a failed pre-dismiss when posting a mention", async () => {
    const page = pageOf({
      composer: locator({
        locator: () => locator({ pressSequentially: async () => undefined }),
        getByRole: () => locator({ first: () => locator({ click: async () => undefined }) }),
      }),
      list: locator({ getByRole: () => locator() }),
    });
    page.page.getByText = (() => locator()) as unknown as Page["getByText"];
    let presses = 0;
    page.page.keyboard.press = (async () => {
      presses += 1;
      if (presses === 1) throw new Error("no focus");
    }) as Page["keyboard"]["press"];
    await new WebNewDriver(page.page).mentionsPostComment("Parity Mention", "hello");
    expect(presses).toBe(2);
  });

  it("keeps @ profile references and drops other profile links", async () => {
    const link = (text: string, href: string | null): Locator =>
      locator({
        allTextContents: async () => [text],
        getAttribute: async (name: string) => {
          expect(name).toBe("href");
          return href;
        },
      });
    const links = [
      link("@Parity Mention", "/acme/profile/m-1"),
      link("Ada Lovelace", "/acme/profile/m-2"),
      link("@Ghost", null),
    ];
    const page = pageOf({
      locator: (selector) =>
        selector === 'a[href*="/profile/"]'
          ? locator({ count: async () => links.length, nth: (index: number) => links[index] as Locator })
          : locator(),
    });
    await expect(new WebNewDriver(page.page).mentionsVisibleReferences()).resolves.toEqual([
      { text: "@Parity Mention", href: "/acme/profile/m-1" },
      { text: "@Ghost", href: null },
    ]);
  });

  it("edits a comment to remove its mention through the menu and save", async () => {
    const steps: string[] = [];
    const editor = locator({
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
      waitFor: async (options?: unknown) =>
        void steps.push(`editor:${String((options as { state?: string }).state ?? "visible")}`),
      locator: (selector: string) => {
        expect(selector).toBe("xpath=ancestor::form[1]");
        return locator({
          getByRole: (role: string, options?: { name?: string }) => {
            expect(role).toBe("button");
            expect(options?.name).toBe("Save comment");
            return locator({ first: () => locator({ click: async () => void steps.push("save") }) });
          },
        });
      },
    });
    const hoverCard = locator({
      hover: async () => void steps.push("hover"),
      locator: (selector: string) => {
        expect(selector).toBe('button[aria-haspopup="menu"]');
        return locator({ first: () => locator({ click: async () => void steps.push("trigger") }) });
      },
    });
    const page = pageOf({
      menuitem: (name: string) =>
        name === "Edit"
          ? locator({ click: async () => void steps.push("menu:Edit") })
          : locator({ waitFor: async () => void steps.push("menu") }),
      locator: (selector) => (selector === '[id^="comment-"] [contenteditable="true"]' ? editor : locator()),
    });
    let articles = 0;
    const inner = page.page.getByRole as (role: string, options?: unknown) => Locator;
    page.page.getByRole = ((role: string, options?: { hasText?: string }) => {
      if (role === "article") {
        articles += 1;
        const wanted = articles === 1 ? "@Parity Mention hi" : "plain words";
        const card = articles === 1 ? hoverCard : locator({ waitFor: async () => void steps.push("card") });
        return locator({
          filter: (filterOptions?: { hasText?: string }) => {
            expect(filterOptions?.hasText).toBe(wanted);
            return locator({ first: () => card });
          },
        });
      }
      return inner(role, options);
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).mentionsEditRemovingMention("@Parity Mention hi", "plain words");
    expect(steps).toEqual([
      "hover",
      "trigger",
      "menu",
      "menu:Edit",
      "type:plain words",
      "save",
      "editor:detached",
      "card",
    ]);
  });

  it("renames the title and blurs back to the feed", async () => {
    const steps: string[] = [];
    const page = pageOf({
      label: (name: string) => {
        expect(name).toBe("Title");
        return locator({
          first: () =>
            locator({
              click: async () => void steps.push("focus"),
              fill: async (value: string) => void steps.push(`fill:${value}`),
            }),
        });
      },
    });
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading") {
        expect(options?.name).toBe("Activity");
        return locator({ click: async () => void steps.push("blur") });
      }
      return locator();
    }) as Page["getByRole"];
    await new WebNewDriver(page.page).activityRenameTitle("Renamed work item");
    expect(steps).toEqual(["focus", "fill:Renamed work item", "blur"]);
  });

  it("counts spied traffic by capability and returns a copy", async () => {
    type Feed = { request: () => { url: () => string; method: () => string }; continue: () => Promise<void> };
    let handler: ((route: Feed) => Promise<void>) | null = null;
    const page = pageOf();
    page.page.route = (async (pattern: string, routeHandler: unknown) => {
      expect(pattern).toBe("**/api/**");
      handler = routeHandler as (route: Feed) => Promise<void>;
    }) as unknown as Page["route"];
    const driver = new WebNewDriver(page.page);
    await driver.activityBeginTrafficSpy({ reload: false });
    const feed = (url: string, method: string): Promise<void> =>
      (handler as unknown as (route: Feed) => Promise<void>)({
        request: () => ({ url: () => url, method: () => method }),
        continue: async () => undefined,
      });
    const api = "https://api.test/api/workspaces/w/projects/p";
    await feed(`${api}/issues/i/history/?activity_type=issue-comment`, "GET");
    await feed(`${api}/issues/i/comments/`, "GET");
    await feed(`${api}/issues/i/comments/`, "POST");
    await feed("https://api.test/api/workspaces/w/entity-search/?query=a", "GET");
    await feed(`${api}/comments/c/reactions/`, "POST");
    await feed(`${api}/comments/c/reactions/`, "GET");
    await feed(`${api}/comments/c/reactions/`, "PUT");
    await feed(`${api}/comments/c/reactions/128077/`, "DELETE");
    await feed(`${api}/comments/c/reactions/128077/`, "GET");
    await feed(`${api}/comments/c/reactions/128077/`, "PUT");
    await feed(`${api}/issues/i/reactions/`, "POST");
    await feed(`${api}/issues/i/reactions/`, "GET");
    await feed(`${api}/issues/i/reactions/`, "PUT");
    await feed(`${api}/issues/i/reactions/128077/`, "DELETE");
    await feed(`${api}/issues/i/reactions/128077/`, "GET");
    await feed(`${api}/issues/i/reactions/128077/`, "PUT");
    await feed(`${api}/issues/i/`, "PATCH");
    await feed(`${api}/issues/i/`, "GET");
    const counts = await driver.activityTrafficCounts();
    expect(counts).toEqual({
      historyReads: 1,
      commentReads: 1,
      commentCreates: 1,
      mentionSearches: 1,
      commentReactionCreates: 1,
      commentReactionDeletes: 1,
      issueReactionCreates: 1,
      issueReactionDeletes: 1,
      unexpectedWrites: 5,
    });
    counts.historyReads = 99;
    expect((await driver.activityTrafficCounts()).historyReads).toBe(1);
  });

  it("reloads the detail on the default spy and reuses one route across windows", async () => {
    const calls: string[] = [];
    const page = pageOf();
    page.page.route = (async () => void calls.push("route")) as unknown as Page["route"];
    page.page.unroute = (async () => void calls.push("unroute")) as unknown as Page["unroute"];
    page.page.reload = (async () => void calls.push("reload")) as Page["reload"];
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "heading") {
        expect(options?.name).toBe("Activity");
        return locator({ waitFor: async () => void calls.push("activity") });
      }
      return locator();
    }) as Page["getByRole"];
    const driver = new WebNewDriver(page.page);
    await driver.activityBeginTrafficSpy();
    await driver.activityBeginTrafficSpy({ reload: false });
    expect(calls).toEqual(["route", "reload", "activity", "unroute", "route"]);
  });

  it("reads the comment reaction control off the card trigger", async () => {
    const page = pageOf({
      cards: () =>
        locator({
          locator: (selector: string) => {
            expect(selector).toBe('button[aria-haspopup="dialog"]');
            return locator({ count: async () => 1 });
          },
        }),
    });
    await expect(new WebNewDriver(page.page).activityCommentReactionControlVisible("hello")).resolves.toBe(true);
    const missing = pageOf({ cards: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing.page).activityCommentReactionControlVisible("hello")).resolves.toBe(false);
  });

  it("reads the issue reaction control and reports a lost row as absent", async () => {
    const page = pageOf();
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "region") {
        expect(options?.name).toBe("Reactions on this work item");
        return locator({ locator: () => locator({ count: async () => 1 }) });
      }
      return locator();
    }) as Page["getByRole"];
    await expect(new WebNewDriver(page.page).issueReactionControlVisible()).resolves.toBe(true);
    const broken = pageOf();
    broken.page.getByRole = (() => {
      throw new Error("gone");
    }) as unknown as Page["getByRole"];
    await expect(new WebNewDriver(broken.page).issueReactionControlVisible()).resolves.toBe(false);
  });

  it("types the draft until it holds", async () => {
    let draft = "";
    const editor = locator({
      pressSequentially: async (value: string) => void (draft = value),
      innerText: async () => draft,
    });
    const page = pageOf({ composer: locator({ locator: () => editor }) });
    await new WebNewDriver(page.page).composerType("hello");
    expect(draft).toBe("hello");
  });

  it("retypes from a selection after a dropped draft", async () => {
    vi.useFakeTimers();
    try {
      let reads = 0;
      const pressed: string[] = [];
      let types = 0;
      const editor = locator({
        press: async (key: string) => void pressed.push(key),
        pressSequentially: async () => void (types += 1),
        innerText: async () => {
          reads += 1;
          return reads > 30 ? "hello" : "";
        },
      });
      const page = pageOf({ composer: locator({ locator: () => editor }) });
      page.page.waitForTimeout = (async (ms: number) => void vi.advanceTimersByTime(ms)) as Page["waitForTimeout"];
      await new WebNewDriver(page.page).composerType("hello");
      expect(pressed).toEqual(["ControlOrMeta+a"]);
      expect(types).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("throws after three attempts that never hold", async () => {
    vi.useFakeTimers();
    try {
      const pressed: string[] = [];
      const editor = locator({
        press: async (key: string) => void pressed.push(key),
        pressSequentially: async () => undefined,
        innerText: async () => "",
      });
      const page = pageOf({ composer: locator({ locator: () => editor }) });
      page.page.waitForTimeout = (async (ms: number) => void vi.advanceTimersByTime(ms)) as Page["waitForTimeout"];
      await expect(new WebNewDriver(page.page).composerType("hello")).rejects.toThrow("never held");
      expect(pressed).toEqual(["ControlOrMeta+a", "ControlOrMeta+a"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("pastes html through the clipboard into the editor", async () => {
    const granted: string[][] = [];
    const clicked: string[] = [];
    const pressed: string[] = [];
    const editor = locator({
      click: async () => void clicked.push("editor"),
      press: async (key: string) => void pressed.push(key),
    });
    const page = pageOf({ composer: locator({ locator: () => editor }) });
    (page.page as unknown as { context: () => { grantPermissions: (p: string[]) => Promise<void> } }).context = () => ({
      grantPermissions: async (p: string[]) => void granted.push(p),
    });
    let seenArg: unknown = null;
    page.page.evaluate = (async (_fn: unknown, arg: unknown) => {
      seenArg = arg;
    }) as Page["evaluate"];
    await new WebNewDriver(page.page).composerPasteHtml("<b>x</b>");
    expect(granted).toEqual([["clipboard-read", "clipboard-write"]]);
    expect(seenArg).toEqual(["<b>x</b>"]);
    expect(clicked).toEqual(["editor"]);
    expect(pressed).toEqual(["ControlOrMeta+v"]);
  });

  it("reads the trimmed draft and the submit arming", async () => {
    const editor = locator({ innerText: async () => "  a draft  " });
    const page = pageOf({
      composer: locator({
        locator: () => editor,
        getByRole: () => locator({ first: () => locator({ isDisabled: async () => true }) }),
      }),
    });
    const driver = new WebNewDriver(page.page);
    await expect(driver.composerDraftText()).resolves.toBe("a draft");
    await expect(driver.composerSubmitDisabled()).resolves.toBe(true);
  });

  it("submits once the button arms", async () => {
    const steps: string[] = [];
    const page = pageOf({
      composer: locator({
        getByRole: (role: string, options?: { name?: string }) => {
          expect(role).toBe("button");
          expect(options?.name).toBe("Comment");
          return locator({
            first: () =>
              locator({
                waitFor: async () => void steps.push("visible"),
                isEnabled: async () => true,
                click: async () => void steps.push("click"),
              }),
          });
        },
      }),
    });
    await new WebNewDriver(page.page).composerSubmit();
    expect(steps).toEqual(["visible", "click"]);
  });

  it("waits for the submit to arm and throws when it never does", async () => {
    vi.useFakeTimers();
    try {
      let arming = 0;
      const page = pageOf({
        composer: locator({
          getByRole: () =>
            locator({
              first: () =>
                locator({
                  isEnabled: async () => {
                    arming += 1;
                    if (arming === 1) throw new Error("detached");
                    return arming > 2;
                  },
                  click: async () => undefined,
                }),
            }),
        }),
      });
      page.page.waitForTimeout = (async (ms: number) => void vi.advanceTimersByTime(ms)) as Page["waitForTimeout"];
      await new WebNewDriver(page.page).composerSubmit();
      expect(arming).toBe(3);
      const stuck = pageOf({
        composer: locator({
          getByRole: () => locator({ first: () => locator({ isEnabled: async () => false }) }),
        }),
      });
      stuck.page.waitForTimeout = (async (ms: number) => void vi.advanceTimersByTime(ms)) as Page["waitForTimeout"];
      await expect(new WebNewDriver(stuck.page).composerSubmit()).rejects.toThrow("never armed");
    } finally {
      vi.useRealTimers();
    }
  });

  it("presses Enter and Shift+Enter from the focused editor", async () => {
    const focused: string[] = [];
    const editor = locator({ focus: async () => void focused.push("editor") });
    const page = pageOf({ composer: locator({ locator: () => editor }) });
    const driver = new WebNewDriver(page.page);
    await driver.composerPressEnter();
    await driver.composerPressShiftEnter();
    expect(focused).toEqual(["editor", "editor"]);
    expect(page.presses).toEqual(["Enter", "Shift+Enter"]);
  });

  it("attaches a file through the composer file input", async () => {
    const files: unknown[] = [];
    const page = pageOf({
      composer: locator({
        locator: (selector: string) => {
          expect(selector).toBe('input[type="file"]');
          return locator({
            first: () => locator({ setInputFiles: async (paths) => void files.push(paths) }),
          });
        },
      }),
    });
    await new WebNewDriver(page.page).composerAttachFile("/tmp/upload.png");
    expect(files).toEqual(["/tmp/upload.png"]);
  });

  it("reads visible comment bodies and skips folded cards", async () => {
    const bodyCard = locator({
      getByRole: (role: string, options?: { name?: RegExp }) => {
        expect(role).toBe("region");
        expect(options?.name?.test("Comment by Ada")).toBe(true);
        return locator({ first: () => locator({ innerText: async () => "  hello  " }) });
      },
    });
    const foldedCard = locator({
      getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }),
    });
    const cards = [bodyCard, foldedCard];
    const page = pageOf();
    page.page.locator = ((selector: string) => {
      expect(selector).toBe('[id^="comment-"]');
      return locator({ count: async () => cards.length, nth: (index: number) => cards[index] as Locator });
    }) as Page["locator"];
    await expect(new WebNewDriver(page.page).composerVisibleCommentTexts()).resolves.toEqual(["hello"]);
  });

  it("opens a comment menu and clicks an item by name", async () => {
    const steps: string[] = [];
    const page = pageOf({
      cards: () =>
        locator({
          hover: async () => void steps.push("hover"),
          locator: () => locator({ first: () => locator({ click: async () => void steps.push("trigger") }) }),
        }),
      menuitem: () => locator({ waitFor: async () => void steps.push("menu") }),
    });
    await new WebNewDriver(page.page).composerOpenCommentMenu("hello");
    expect(steps).toEqual(["hover", "trigger", "menu"]);
    const clicked: string[] = [];
    const picker = pageOf();
    picker.page.getByRole = ((role: string, options?: { name?: string }) => {
      expect(role).toBe("menuitem");
      return locator({ click: async () => void clicked.push(String(options?.name)) });
    }) as Page["getByRole"];
    await new WebNewDriver(picker.page).composerMenuClick("Delete");
    expect(clicked).toEqual(["Delete"]);
  });

  it("types into the open edit form and saves through the check", async () => {
    const steps: string[] = [];
    const editor = locator({
      press: async (key: string) => void steps.push(`press:${key}`),
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
      locator: () =>
        locator({
          getByRole: (role: string, options?: { name?: string }) =>
            locator({
              first: () =>
                locator({
                  click: async () => void steps.push(`click:${String(options?.name)}`),
                  isDisabled: async () => options?.name !== "Save comment",
                }),
            }),
        }),
    });
    const page = pageOf({
      locator: (selector) => (selector === '[id^="comment-"] [contenteditable="true"]' ? editor : locator()),
    });
    const driver = new WebNewDriver(page.page);
    await driver.composerEditType("revised words");
    await expect(driver.composerEditSaveDisabled()).resolves.toBe(false);
    await driver.composerEditSave();
    await driver.composerEditDiscard();
    expect(steps).toEqual([
      "press:ControlOrMeta+a",
      "type:revised words",
      "click:Save comment",
      "click:Cancel editing",
    ]);
  });

  it("appends to the edit draft without selecting it first", async () => {
    const steps: string[] = [];
    const editor = locator({
      click: async () => void steps.push("click"),
      press: async (key: string) => void steps.push(`press:${key}`),
      pressSequentially: async (value: string) => void steps.push(`type:${value}`),
    });
    const page = pageOf({
      locator: (selector) => (selector === '[id^="comment-"] [contenteditable="true"]' ? editor : locator()),
    });
    await new WebNewDriver(page.page).composerEditAppend(" kept");
    expect(steps).toEqual(["click", "press:ControlOrMeta+Home", "press:End", "type: kept"]);
  });

  it("presses Enter from the focused edit form", async () => {
    const focused: string[] = [];
    const page = pageOf({
      locator: (selector) =>
        selector === '[id^="comment-"] [contenteditable="true"]'
          ? locator({ focus: async () => void focused.push("editor") })
          : locator(),
    });
    await new WebNewDriver(page.page).composerEditPressEnter();
    expect(focused).toEqual(["editor"]);
    expect(page.presses).toEqual(["Enter"]);
  });

  it("reads card author, time and the exact-time tooltip", async () => {
    const waits: string[] = [];
    const moves: number[][] = [];
    const timeButton = locator({
      innerText: async () => "3 minutes ago",
      hover: async () => void waits.push("hover"),
    });
    const page = pageOf({
      cards: () =>
        locator({
          evaluate: async (fn: (element: never) => unknown) => fn({ scrollIntoView: () => undefined } as never),
          locator: (selector: string) => {
            expect(selector).toBe("header div span");
            return locator({ first: () => locator({ innerText: async () => "  Ada Lovelace  " }) });
          },
          getByRole: (role: string, options?: { name?: RegExp }) => {
            expect(role).toBe("button");
            expect(options?.name?.test("Commented 3 minutes ago")).toBe(true);
            return locator({ first: () => timeButton });
          },
        }),
      tooltip: locator({
        first: () =>
          locator({
            waitFor: async (options?: unknown) => void waits.push((options as { state?: string }).state ?? "attached"),
            innerText: async () => "  October 29, 2026  ",
          }),
      }),
    });
    page.page.mouse = { move: async (x: number, y: number) => void moves.push([x, y]) } as Page["mouse"];
    await expect(new WebNewDriver(page.page).composerCommentMeta("hello")).resolves.toEqual({
      author: "Ada Lovelace",
      time: "3 minutes ago",
      edited: false,
      tooltip: "October 29, 2026",
    });
    expect(waits).toEqual(["hover", "attached", "detached"]);
    expect(moves).toEqual([[8, 8]]);
  });

  it("reports unreadable author, time and tooltip as empty", async () => {
    const page = pageOf({
      cards: () =>
        locator({
          locator: () => locator({ first: () => locator({ innerText: () => Promise.reject(new Error("gone")) }) }),
          getByRole: () => locator({ first: () => locator({ hover: () => Promise.reject(new Error("gone")) }) }),
        }),
      tooltip: locator({
        first: () =>
          locator({
            waitFor: () => Promise.reject(new Error("gone")),
            innerText: async () => "",
          }),
      }),
    });
    await expect(new WebNewDriver(page.page).composerCommentMeta("hello")).resolves.toEqual({
      author: "",
      time: "",
      edited: false,
      tooltip: null,
    });
  });

  it("reads an empty tooltip as null and an edited time as edited", async () => {
    const page = pageOf({
      cards: () =>
        locator({
          locator: () => locator({ first: () => locator({ innerText: async () => "Ada" }) }),
          getByRole: () =>
            locator({
              first: () => locator({ innerText: async () => "2 days ago (edited)", hover: async () => undefined }),
            }),
        }),
      tooltip: locator({ first: () => locator({ innerText: async () => "   " }) }),
    });
    await expect(new WebNewDriver(page.page).composerCommentMeta("hello")).resolves.toEqual({
      author: "Ada",
      time: "2 days ago (edited)",
      edited: true,
      tooltip: null,
    });
  });

  it("counts body images without the header avatar", async () => {
    const page = pageOf({
      cards: () =>
        locator({
          getByRole: (role: string, options?: { name?: RegExp }) => {
            expect(role).toBe("region");
            expect(options?.name?.test("Comment by Ada")).toBe(true);
            return locator({
              locator: (selector: string) => {
                expect(selector).toBe("img");
                return locator({ count: async () => 2 });
              },
            });
          },
        }),
    });
    await expect(new WebNewDriver(page.page).composerCommentImageCount("hello")).resolves.toBe(2);
  });

  it("classifies visible notices by the product wording", async () => {
    const toast = (text: string | Error): Locator =>
      locator({
        innerText: async () => {
          if (text instanceof Error) throw text;
          return text;
        },
      });
    const toasts = [
      toast("Comment created successfully"),
      toast("Couldn't add the comment\nrefused"),
      toast(""),
      toast("Something odd"),
      toast(new Error("gone")),
    ];
    const page = pageOf({
      locator: (selector) => {
        expect(selector).toBe('[data-testid="toast-notification"]');
        return locator({ count: async () => toasts.length, nth: (index: number) => toasts[index] as Locator });
      },
    });
    await expect(new WebNewDriver(page.page).composerVisibleNotices()).resolves.toEqual([
      { message: "Comment created successfully", kind: "success" },
      { message: "Couldn't add the comment\nrefused", kind: "error" },
      { message: "Something odd", kind: "unknown" },
    ]);
  });

  it("reads rule card bodies and reports folded cards as null", async () => {
    const withBody = locator({
      getByRole: (role: string, options?: { name?: RegExp }) => {
        expect(role).toBe("region");
        expect(options?.name?.test("Comment by Ada")).toBe(true);
        return locator({ first: () => locator({ innerText: async () => "  hello  " }) });
      },
    });
    const emptyBody = locator({
      getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }),
      getByText: (text: string) => {
        expect(text).toBe("This comment has no text.");
        return locator({ first: () => locator({ innerText: async () => "This comment has no text." }) });
      },
    });
    const folded = locator({
      getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }),
      getByText: () => locator({ first: () => locator({ count: async () => 0 }) }),
    });
    for (const [card, expected] of [
      [withBody, "hello"],
      [emptyBody, "This comment has no text."],
      [folded, null],
    ] as const) {
      const page = pageOf({
        locator: (selector) => {
          expect(selector).toBe("#comment-c1");
          return card;
        },
      });
      await expect(new WebNewDriver(page.page).rulesCommentBodyText("c1")).resolves.toBe(expected);
    }
  });

  it("picks rule card menu options by stable key", async () => {
    const cases: [RulesCommentMenuOption, string | RegExp][] = [
      ["edit", "Edit"],
      ["copy_link", "Copy link"],
      ["access_switch", /Switch to (public|private) comment/],
      ["fold", "Fold comment"],
      ["unfold", "Unfold comment"],
      ["delete", "Delete"],
    ];
    for (const [option, title] of cases) {
      const granted: string[][] = [];
      const clicked: unknown[] = [];
      const page = pageOf({
        locator: (selector) => {
          expect(selector).toBe("#comment-c1");
          return locator({
            locator: (inner: string) => {
              expect(inner).toBe('button[aria-haspopup="menu"]');
              return locator({ first: () => locator() });
            },
          });
        },
      });
      (page.page as unknown as { context: () => { grantPermissions: (p: string[]) => Promise<void> } }).context =
        () => ({ grantPermissions: async (p: string[]) => void granted.push(p) });
      page.page.getByRole = ((role: string, options?: { name?: string | RegExp }) => {
        expect(role).toBe("menuitem");
        clicked.push(options?.name);
        return locator({ click: async () => undefined });
      }) as Page["getByRole"];
      await new WebNewDriver(page.page).rulesChooseCommentMenuOption("c1", option);
      expect(clicked).toEqual([title]);
      expect(granted).toEqual(option === "copy_link" ? [["clipboard-read", "clipboard-write"]] : []);
    }
  });

  it("opens deep links and reloads through the feed wait", async () => {
    const calls: string[] = [];
    const page = pageOf();
    page.page.getByRole = ((role: string, options?: { name?: string }) => {
      if (role === "group") {
        expect(options?.name).toBe("Add comment");
        return locator({
          first: () =>
            locator({
              waitFor: async (wait?: unknown) =>
                void calls.push(`feed:${String((wait as { timeout?: number }).timeout)}`),
            }),
        });
      }
      return locator();
    }) as Page["getByRole"];
    page.page.goto = (async (url: string) => void calls.push(`goto:${url}`)) as Page["goto"];
    page.page.reload = (async () => void calls.push("reload")) as Page["reload"];
    page.page.waitForLoadState = (async (state: string) =>
      void calls.push(`load:${state}`)) as Page["waitForLoadState"];
    const driver = new WebNewDriver(page.page);
    await driver.rulesOpenDeepLink("https://x/acme/browse/WEB-1#comment-c1");
    await driver.rulesReload();
    await driver.rulesOpenIssueDetailRaw("acme", "p-1", "i-1");
    expect(calls).toEqual([
      "goto:https://x/acme/browse/WEB-1#comment-c1",
      "load:domcontentloaded",
      "feed:120000",
      "reload",
      "load:domcontentloaded",
      "feed:120000",
      "goto:/acme/projects/p-1/issues/i-1",
      "load:domcontentloaded",
    ]);
  });

  it("reads the deep-link highlight off the card wrapper", async () => {
    const highlighted = pageOf({
      locator: (selector) => {
        expect(selector).toBe("#comment-c1");
        return locator({
          evaluate: async (fn: (element: never) => unknown) =>
            fn({ parentElement: { className: "rounded ring-1 ring-(--accent)" } } as never),
        });
      },
    });
    await expect(new WebNewDriver(highlighted.page).rulesCommentHighlighted("c1")).resolves.toBe(true);
    const plain = pageOf({
      locator: () =>
        locator({ evaluate: async (fn: (element: never) => unknown) => fn({ parentElement: null } as never) }),
    });
    await expect(new WebNewDriver(plain.page).rulesCommentHighlighted("c1")).resolves.toBe(false);
    const gone = pageOf({
      locator: () => locator({ evaluate: async () => Promise.reject(new Error("detached")) }),
    });
    await expect(new WebNewDriver(gone.page).rulesCommentHighlighted("c1")).resolves.toBe(false);
  });

  it("reads the rule access mark or reports it hidden", async () => {
    const mark = (label: string | null): Locator =>
      locator({
        locator: (selector: string) => {
          expect(selector).toBe('span[aria-label="Private comment"], span[aria-label="Public comment"]');
          return locator({
            first: () => locator({ count: async () => (label === null ? 0 : 1), getAttribute: async () => label }),
          });
        },
      });
    for (const [label, expected] of [
      ["Public comment", "public"],
      ["Private comment", "internal"],
      [null, "hidden"],
    ] as const) {
      const page = pageOf({
        locator: (selector) => {
          expect(selector).toBe("#comment-c1");
          return mark(label);
        },
      });
      await expect(new WebNewDriver(page.page).rulesCommentAccessBadge("c1")).resolves.toBe(expected);
    }
    const strange = pageOf({ locator: () => mark("Something else") });
    await expect(new WebNewDriver(strange.page).rulesCommentAccessBadge("c1")).rejects.toThrow("no visibility mark");
  });

  it("waits for the missing-item state on refused details", async () => {
    const waits: string[] = [];
    const page = pageOf();
    page.page.getByText = ((text: string) => {
      expect(text).toBe("No work item matches this address");
      return locator({
        first: () => locator({ waitFor: async () => void waits.push("missing"), isVisible: async () => true }),
      });
    }) as unknown as Page["getByText"];
    await expect(new WebNewDriver(page.page).rulesIssueMissingVisible()).resolves.toBe(true);
    expect(waits).toEqual(["missing"]);
  });

  it("reads composer presence and rule card presence and text", async () => {
    const withComposer = pageOf({ composer: locator({ count: async () => 1 }) });
    await expect(new WebNewDriver(withComposer.page).rulesCommentComposerVisible()).resolves.toBe(true);
    const withoutComposer = pageOf({ composer: locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(withoutComposer.page).rulesCommentComposerVisible()).resolves.toBe(false);
    const withCard = pageOf({
      locator: (selector) => {
        expect(selector).toBe("#comment-c1");
        return locator({ innerText: async () => "  hello  " });
      },
    });
    const driver = new WebNewDriver(withCard.page);
    await expect(driver.rulesCommentCardVisible("c1")).resolves.toBe(true);
    await expect(driver.rulesCommentCardText("c1")).resolves.toBe("hello");
    const withoutCard = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(withoutCard.page).rulesCommentCardVisible("c1")).resolves.toBe(false);
  });
});
