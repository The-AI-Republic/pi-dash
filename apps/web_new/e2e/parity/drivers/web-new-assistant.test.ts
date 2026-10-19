// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver assistant entry-surface methods
// (NEWFRONT-603, AGT-038–041/048). A stub Page stands in for Playwright;
// the assertions are about the address each method opens, the selector it
// reads and the route stub it installs — never about a real browser.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  press?: (key: string) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
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
    isEnabled: async () => true,
    isDisabled: async () => false,
    click: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    inputValue: async () => "",
    allTextContents: async () => [],
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
    url: () => "https://app.test/acme/assistant",
    getByRole: vi.fn(() => locator()),
    getByText: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    route: vi.fn(async () => undefined),
    unroute: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as Page;
}

/** The last route handler installed for a pattern. */
function handlerFor(route: ReturnType<typeof vi.fn>, pattern: string) {
  const call = route.mock.calls.find((args) => args[0] === pattern);
  if (call === undefined) throw new Error(`no route installed for ${pattern}`);
  return call[1] as (route: Route) => Promise<void>;
}

function stubRoute(method: string, url: string) {
  return {
    request: () => ({ method: () => method, url: () => url }),
    fallback: vi.fn(async () => undefined),
    fulfill: vi.fn(async (_options: unknown) => undefined),
  };
}

describe("assistant navigation methods", () => {
  it("opens the landing, settles the load state and waits for the resolved column", async () => {
    const goto = vi.fn(async () => null);
    const waitForLoadState = vi.fn(async () => undefined);
    const columnWait = vi.fn(async (_options?: unknown) => undefined);
    const settledWait = vi.fn(async (_options?: unknown) => undefined);
    const page = pageOf({
      goto,
      waitForLoadState,
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-landing"]') return locator({ waitFor: columnWait });
        if (selector === '[data-testid="assistant-landing"][data-settled="true"]') {
          return locator({ waitFor: settledWait });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.assistantOpenLanding("acme");

    expect(goto).toHaveBeenCalledWith("/acme/assistant");
    expect(waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    // The column alone paints while the config read is pending, so the open
    // must also wait for the settled marker (AGT-038–041 cold open).
    expect(columnWait).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(settledWait).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("opens a conversation by id and waits for its settled state", async () => {
    const goto = vi.fn(async () => null);
    const threadWait = vi.fn(async (_options?: unknown) => undefined);
    const settledWait = vi.fn(async (_options?: unknown) => undefined);
    const page = pageOf({
      goto,
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-thread"]') return locator({ waitFor: threadWait });
        if (selector === '[data-testid="assistant-thread"][data-settled="true"]') {
          return locator({ waitFor: settledWait });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.assistantOpenThread("acme", "thread-1");

    expect(goto).toHaveBeenCalledWith("/acme/assistant/thread-1");
    expect(threadWait).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(settledWait).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("opens the workspace home and settles on the main region", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async (_options?: unknown) => undefined);
    const getByRole = vi.fn((_role: string) => locator({ waitFor }));
    const driver = new WebNewDriver(pageOf({ goto, getByRole }));

    await driver.assistantOpenHome("acme");

    expect(goto).toHaveBeenCalledWith("/acme");
    expect(getByRole).toHaveBeenCalledWith("main");
    expect(waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reports the pathname and goes back through history", async () => {
    const goBack = vi.fn(async () => null);
    const waitForLoadState = vi.fn(async () => undefined);
    const driver = new WebNewDriver(
      pageOf({ goBack, waitForLoadState, url: () => "https://app.test/acme/assistant/t1?x=1" })
    );

    await expect(driver.assistantCurrentPath()).resolves.toBe("/acme/assistant/t1");
    await driver.assistantGoBack();

    expect(goBack).toHaveBeenCalledTimes(1);
    expect(waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
  });
});

describe("assistant landing reads", () => {
  it("reads the headline and caption from the landing column", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector !== '[data-testid="assistant-landing"]') return locator();
        return locator({
          locator: (inner: string) => {
            if (inner === "h1") return locator({ innerText: async () => " How can I help you today? " });
            if (inner === "p") return locator({ innerText: async () => " Ask about your issues. " });
            return locator();
          },
        });
      },
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantLandingGreeting()).resolves.toEqual({
      headline: "How can I help you today?",
      caption: "Ask about your issues.",
    });
  });

  it("reports no greeting when the column has no headline", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-landing"]'
          ? locator({
              locator: (inner: string) => (inner === "h1" ? locator({ count: async () => 0 }) : locator()),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantLandingGreeting()).resolves.toBeNull();
  });

  it("returns an empty caption when the column has no paragraph", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-landing"]'
          ? locator({
              locator: (inner: string) =>
                inner === "p" ? locator({ count: async () => 0 }) : locator({ innerText: async () => "Hello" }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantLandingGreeting()).resolves.toEqual({ headline: "Hello", caption: "" });
  });

  it("reports composer visibility from the landing's textarea", async () => {
    const isVisible = vi.fn(async () => true);
    const selectors: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        selectors.push(selector);
        return locator({ isVisible, count: async () => 1 });
      },
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantLandingComposerVisible()).resolves.toBe(true);
    expect(selectors[0]).toContain('[data-testid="assistant-landing"] textarea');
    expect(isVisible).toHaveBeenCalledTimes(1);
  });

  it("reports the composer hidden when the selector matches nothing", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));

    await expect(driver.assistantLandingComposerVisible()).resolves.toBe(false);
  });
});

describe("assistantSetupCard", () => {
  it("returns null when the setup title is absent", async () => {
    const driver = new WebNewDriver(pageOf({ getByText: () => locator({ count: async () => 0 }) }));

    await expect(driver.assistantSetupCard()).resolves.toBeNull();
  });

  it("reads title, body and action from the setup card root", async () => {
    const getByRole = vi.fn((_role: string, options?: unknown) => {
      expect(options).toEqual({ name: "Configure provider", exact: true });
      return locator({ innerText: async () => " Configure provider " });
    });
    const page = pageOf({
      getByText: () => locator({ innerText: async () => " Set up your AI assistant " }),
      locator: (selector: string) =>
        selector === '[data-testid="assistant-setup-card"]'
          ? locator({
              getByRole,
              locator: (inner: string) =>
                inner === "p" ? locator({ innerText: async () => " Bring your own LLM provider key. " }) : locator(),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSetupCard()).resolves.toEqual({
      title: "Set up your AI assistant",
      body: "Bring your own LLM provider key.",
      button: "Configure provider",
    });
  });

  it("falls back to the title's parent and the action label when the card is absent", async () => {
    const page = pageOf({
      getByText: () =>
        locator({
          innerText: async () => "Set up your AI assistant",
          locator: (selector: string) =>
            selector === "xpath=.."
              ? locator({
                  getByRole: () => locator({ innerText: async () => Promise.reject(new Error("gone")) }),
                  locator: () => locator({ innerText: async () => Promise.reject(new Error("gone")) }),
                })
              : locator(),
        }),
      locator: (selector: string) =>
        selector === '[data-testid="assistant-setup-card"]' ? locator({ count: async () => 0 }) : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSetupCard()).resolves.toEqual({
      title: "Set up your AI assistant",
      body: "",
      button: "",
    });
  });

  it("clicks the setup card's provider action", async () => {
    const click = vi.fn(async () => undefined);
    const getByRole = vi.fn((_role: string, _options?: unknown) => locator({ click }));
    const driver = new WebNewDriver(
      pageOf({ locator: (_selector: string) => locator({ getByRole, count: async () => 1 }) })
    );

    await driver.assistantSetupCardClick();

    expect(getByRole).toHaveBeenCalledWith("button", { name: "Configure provider", exact: true });
    expect(click).toHaveBeenCalledTimes(1);
  });
});

describe("assistant composer methods", () => {
  it("fills the draft box and reads it back", async () => {
    const fill = vi.fn(async () => undefined);
    const box = locator({ fill, inputValue: async () => "hello" });
    const selectors: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        selectors.push(selector);
        return selector.includes("textarea") ? box : locator();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.assistantFillDraft("hello");
    await expect(driver.assistantDraftValue()).resolves.toBe("hello");

    expect(fill).toHaveBeenCalledWith("hello");
    expect(selectors[0]).toBe('textarea[placeholder*="Ask Pi Dash to do something"]');
  });

  it("presses the three submit keys on the draft box", async () => {
    const press = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) => (selector.includes("textarea") ? locator({ press }) : locator()),
    });
    const driver = new WebNewDriver(page);

    await driver.assistantPressEnter();
    await driver.assistantPressShiftEnter();
    await driver.assistantPressControlEnter();

    expect(press.mock.calls.map((call) => call[0])).toEqual(["Enter", "Shift+Enter", "Control+Enter"]);
  });

  it("reads send visibility, enabled state and clicks send", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'button[aria-label="Send message"]'
          ? locator({ click, isEnabled: async () => false, count: async () => 1 })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSendVisible()).resolves.toBe(true);
    await expect(driver.assistantSendEnabled()).resolves.toBe(false);
    await driver.assistantClickSend();

    expect(click).toHaveBeenCalledTimes(1);
  });

  it("reports send disabled and hidden when the button is absent", async () => {
    const driver = new WebNewDriver(
      pageOf({
        locator: () =>
          locator({
            count: async () => 0,
            isVisible: async () => false,
            isEnabled: async () => Promise.reject(new Error("no element")),
          }),
      })
    );

    await expect(driver.assistantSendEnabled()).resolves.toBe(false);
    await expect(driver.assistantSendVisible()).resolves.toBe(false);
  });

  it("hides the stop action while the composer is gated", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));

    await expect(driver.assistantStopVisible()).resolves.toBe(false);
  });

  it("reads the lock reason and the textarea disabled state", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-composer-reason"]')
          return locator({ innerText: async () => " Please set your API key. " });
        if (selector.includes("textarea")) return locator({ isDisabled: async () => true });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantComposerReason()).resolves.toBe("Please set your API key.");
    await expect(driver.assistantTextareaDisabled()).resolves.toBe(true);
  });

  it("treats an absent or blank reason as no reason", async () => {
    const absent = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    await expect(absent.assistantComposerReason()).resolves.toBeNull();

    const blank = new WebNewDriver(pageOf({ locator: () => locator({ innerText: async () => "   " }) }));
    await expect(blank.assistantComposerReason()).resolves.toBeNull();
  });

  it("reads the last toast's title and message lines", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="toast-notification"]'
          ? locator({ innerText: async () => "Unable to start chat\nThe assistant refused.\n" })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantLastToast()).resolves.toEqual({
      title: "Unable to start chat",
      message: "The assistant refused.",
    });
  });

  it("returns no toast when none is shown or the text is blank", async () => {
    const absent = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    await expect(absent.assistantLastToast()).resolves.toBeNull();

    const blank = new WebNewDriver(pageOf({ locator: () => locator({ innerText: async () => "  \n " }) }));
    await expect(blank.assistantLastToast()).resolves.toBeNull();
  });
});

describe("assistant API spy and thread-create stubs", () => {
  it("classifies assistant requests and passes them through", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));

    await driver.assistantStartApiSpy();
    const handler = handlerFor(route, "**/api/workspaces/*/ai-assistant/**");
    const cases: [string, string][] = [
      ["POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/"],
      ["POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/"],
      ["POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/cancel/"],
      ["GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/"],
      ["GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/"],
      ["DELETE", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/"],
    ];
    for (const [method, url] of cases) {
      const stub = stubRoute(method, url);
      await handler(stub as unknown as Route);
      expect(stub.fallback).toHaveBeenCalledTimes(1);
    }

    await expect(driver.assistantApiCounts()).resolves.toEqual({
      threadCreate: 1,
      send: 1,
      cancel: 1,
      threadList: 1,
      messageList: 1,
    });
  });

  it("resets the counters on a fresh spy and clears the route", async () => {
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, unroute }));

    await driver.assistantStartApiSpy();
    await handlerFor(
      route,
      "**/api/workspaces/*/ai-assistant/**"
    )(stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/") as unknown as Route);
    await driver.assistantStartApiSpy();

    expect((await driver.assistantApiCounts()).threadCreate).toBe(0);
    await driver.assistantStopApiSpy();
    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/ai-assistant/**");
  });

  it("fails one thread create and then falls through", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));

    await driver.assistantFailThreadCreateOnce();
    const handler = handlerFor(route, "**/api/workspaces/*/ai-assistant/threads/");

    const first = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/");
    await handler(first as unknown as Route);
    expect(first.fulfill).toHaveBeenCalledWith({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "parity thread-create failure" }),
    });
    expect(first.fallback).not.toHaveBeenCalled();

    const second = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/");
    await handler(second as unknown as Route);
    expect(second.fulfill).not.toHaveBeenCalled();
    expect(second.fallback).toHaveBeenCalledTimes(1);
  });

  it("passes non-create requests through the failure stub untouched", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));

    await driver.assistantFailThreadCreateOnce();
    const handler = handlerFor(route, "**/api/workspaces/*/ai-assistant/threads/");
    const read = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/");
    await handler(read as unknown as Route);

    expect(read.fulfill).not.toHaveBeenCalled();
    expect(read.fallback).toHaveBeenCalledTimes(1);
  });

  it("delays exactly one thread create and then falls through", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));

    await driver.assistantDelayThreadCreate(0);
    const handler = handlerFor(route, "**/api/workspaces/*/ai-assistant/threads/");
    const first = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/");
    await handler(first as unknown as Route);
    const second = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/");
    await handler(second as unknown as Route);
    const foreign = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/");
    await handler(foreign as unknown as Route);

    expect(first.fallback).toHaveBeenCalledTimes(1);
    expect(second.fallback).toHaveBeenCalledTimes(1);
    expect(foreign.fallback).toHaveBeenCalledTimes(1);
  });

  it("clears both thread-create stubs", async () => {
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ unroute }));

    await driver.assistantClearThreadCreateStubs();

    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/ai-assistant/threads/");
  });
});

describe("assistant conversation rail", () => {
  it("reads each thread link's title, href and active marker", async () => {
    const links = [
      locator({
        innerText: async () => " First ",
        getAttribute: async (name: string) => (name === "href" ? "/acme/assistant/1" : "page"),
      }),
      locator({
        innerText: async () => "Second",
        getAttribute: async (name: string) => (name === "href" ? "/acme/assistant/2" : null),
      }),
    ];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-rail"]'
          ? locator({
              locator: (inner: string) =>
                inner === "nav a"
                  ? locator({ count: async () => links.length, nth: (i: number) => links[i]! })
                  : locator(),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSidebarThreads()).resolves.toEqual([
      { title: "First", href: "/acme/assistant/1", active: true },
      { title: "Second", href: "/acme/assistant/2", active: false },
    ]);
  });

  it("reports an empty rail with no placeholder when the list is absent", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-rail"]'
          ? locator({
              locator: (inner: string) => (inner === "nav a" ? locator({ count: async () => 0 }) : locator()),
              getByText: () => locator({ isVisible: async () => Promise.reject(new Error("no placeholder")) }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSidebarThreads()).resolves.toEqual([]);
    await expect(driver.assistantSidebarEmptyVisible()).resolves.toBe(false);
  });

  it("reports the empty rail placeholder when it is shown", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-rail"]'
          ? locator({ getByText: () => locator({ isVisible: async () => true }) })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantSidebarEmptyVisible()).resolves.toBe(true);
  });

  it("clicks the new-chat link and a thread by index", async () => {
    const newChatClick = vi.fn(async () => undefined);
    const threadClick = vi.fn(async () => undefined);
    const links = [locator({ click: newChatClick }), locator({ click: threadClick })];
    const getByRole = vi.fn((_role: string, options?: unknown) => {
      expect(options).toEqual({ name: "New chat" });
      return links[0]!;
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-rail"]'
          ? locator({
              getByRole,
              locator: (inner: string) => (inner === "nav a" ? locator({ nth: (i: number) => links[i]! }) : locator()),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await driver.assistantClickNewChat();
    await driver.assistantClickSidebarThread(1);

    expect(getByRole).toHaveBeenCalledTimes(1);
    expect(newChatClick).toHaveBeenCalledTimes(1);
    expect(threadClick).toHaveBeenCalledTimes(1);
  });
});

describe("assistant dashboard card", () => {
  it("reports the card visible only when its input exists", async () => {
    const shown = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 1 }) }));
    await expect(shown.assistantCardVisible()).resolves.toBe(true);

    const hidden = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    await expect(hidden.assistantCardVisible()).resolves.toBe(false);
  });

  it("fills and reads the card draft, and presses Enter", async () => {
    const fill = vi.fn(async () => undefined);
    const press = vi.fn(async () => undefined);
    const input = locator({ fill, press, inputValue: async () => "draft" });
    const page = pageOf({
      locator: (selector: string) => (selector.includes("input[placeholder") ? input : locator()),
    });
    const driver = new WebNewDriver(page);

    await driver.assistantCardFillDraft("draft");
    await expect(driver.assistantCardDraftValue()).resolves.toBe("draft");
    await driver.assistantCardPressEnter();

    expect(fill).toHaveBeenCalledWith("draft");
    expect(press).toHaveBeenCalledWith("Enter");
  });

  it("clicks Ask, reports its disabled state and clicks a suggestion", async () => {
    const askClick = vi.fn(async () => undefined);
    const chipClick = vi.fn(async () => undefined);
    const getByRole = vi.fn((_role: string, options?: unknown) => {
      const name = (options as { name?: string }).name;
      return name === "Ask"
        ? locator({ click: askClick, isDisabled: async () => true })
        : locator({ click: chipClick });
    });
    const page = pageOf({
      locator: (selector: string) => (selector === "#home-assistant-card" ? locator({ getByRole }) : locator()),
    });
    const driver = new WebNewDriver(page);

    await driver.assistantCardClickAsk();
    await expect(driver.assistantCardAskDisabled()).resolves.toBe(true);
    await driver.assistantCardClickSuggestion("What's assigned to me?");

    expect(askClick).toHaveBeenCalledTimes(1);
    expect(chipClick).toHaveBeenCalledTimes(1);
    expect(getByRole).toHaveBeenCalledWith("button", { name: "Ask", exact: true });
    expect(getByRole).toHaveBeenCalledWith("button", { name: "What's assigned to me?", exact: true });
  });

  it("reads the suggestion chips in order, trimmed", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === "#home-assistant-card"
          ? locator({ locator: () => locator({ allTextContents: async () => [" One ", "Two", "  "] }) })
          : locator(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantCardSuggestions()).resolves.toEqual(["One", "Two"]);
  });

  it("returns no suggestions when the card is absent", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));

    await expect(driver.assistantCardSuggestions()).resolves.toEqual([]);
  });

  it("reads the recents' titles and hrefs from the recents list only", async () => {
    const links = [
      locator({ innerText: async () => " Newest ", getAttribute: async () => "/acme/assistant/2" }),
      locator({ innerText: async () => "Older", getAttribute: async () => null }),
    ];
    const scoped = vi.fn((selector: string) =>
      selector === "ul a"
        ? locator({ count: async () => links.length, nth: (i: number) => links[i]! })
        : // Any broader scope would also pick up the keyless setup link.
          locator({ count: async () => links.length + 1, nth: () => links[0]! })
    );
    const page = pageOf({
      locator: (selector: string) => (selector === "#home-assistant-card" ? locator({ locator: scoped }) : locator()),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.assistantCardRecents()).resolves.toEqual([
      { title: "Newest", href: "/acme/assistant/2" },
      { title: "Older", href: "" },
    ]);
    expect(scoped).toHaveBeenCalledWith("ul a");
  });

  it("returns no recents when the card is absent", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));

    await expect(driver.assistantCardRecents()).resolves.toEqual([]);
  });

  it("clicks a recent by index inside the recents list", async () => {
    const click = vi.fn(async () => undefined);
    const scoped = vi.fn((_selector: string) => locator({ nth: () => locator({ click }) }));
    const page = pageOf({
      locator: (selector: string) => (selector === "#home-assistant-card" ? locator({ locator: scoped }) : locator()),
    });
    const driver = new WebNewDriver(page);

    await driver.assistantCardClickRecent(1);

    expect(scoped).toHaveBeenCalledWith("ul a");
    expect(click).toHaveBeenCalledTimes(1);
  });
});
