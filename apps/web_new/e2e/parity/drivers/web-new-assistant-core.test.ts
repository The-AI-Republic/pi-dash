// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver thread-view methods (NEWFRONT-605,
// AGT-042/043/047/049/053/054). A stub Page stands in for Playwright: the
// assertions are about the selectors each read uses, the frames the stream
// stub serves and the request shapes the one-shot delays install.
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
  evaluate?: (fn: (node: Element) => unknown) => Promise<unknown>;
  innerHTML?: () => Promise<string>;
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
    evaluate: async () => 0,
    innerHTML: async () => "",
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
    abort: vi.fn(async () => undefined),
  };
}

describe("transcript reads", () => {
  it("reads rows by role and skips empty bubbles", async () => {
    const rows = [
      locator({ getAttribute: async () => "user", innerText: async () => " hello " }),
      locator({ getAttribute: async () => "assistant", innerText: async () => "" }),
      locator({ getAttribute: async () => "error", innerText: async () => " Could not reach. " }),
    ];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({
              locator: (inner: string) =>
                inner === ":scope > li"
                  ? locator({ count: async () => rows.length, nth: (i) => rows[i] as never })
                  : locator(),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.assistantBubbles()).resolves.toEqual([
      { role: "user", text: "hello" },
      { role: "error", text: "Could not reach." },
    ]);
  });

  it("reports tool calls and tool results as one tool role", async () => {
    // The parity contract has one `tool` role; the transcript keeps the two
    // stored roles apart for styling and deep links.
    const rows = [
      locator({ getAttribute: async () => "tool_call", innerText: async () => "Searching" }),
      locator({ getAttribute: async () => "tool_result", innerText: async () => "Found it" }),
      locator({ getAttribute: async () => "assistant", innerText: async () => "Done" }),
    ];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({
              locator: (inner: string) =>
                inner === ":scope > li"
                  ? locator({ count: async () => rows.length, nth: (i) => rows[i] as never })
                  : locator(),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).assistantBubbles()).resolves.toEqual([
      { role: "tool", text: "Searching" },
      { role: "tool", text: "Found it" },
      { role: "assistant", text: "Done" },
    ]);
  });

  it("returns no bubbles when the transcript is absent", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    await expect(driver.assistantBubbles()).resolves.toEqual([]);
  });

  it("reads one bubble's rendered HTML by index", async () => {
    const evaluate = vi.fn(async (fn: (node: Element) => unknown) =>
      fn({
        outerHTML: '<div data-role="assistant" class="text-(--danger)"><h1>Styled</h1></div>',
      } as unknown as Element)
    );
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({ locator: () => locator({ locator: () => locator({ evaluate }) }) })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    const html = await driver.assistantBubbleHtml(0);
    expect(html).toBe('<div data-role="assistant" class="text-(--danger)"><h1>Styled</h1></div>');
    // The tint lives on the bubble element, so the read must carry it.
    expect(html).toContain("text-(--danger)");
  });

  it("reads an error row's rendered colour, not its class name", async () => {
    // The two apps spell the danger token differently, so the probe reads
    // what the reader sees; a class name would lock in one app's spelling.
    const evaluate = vi.fn(async (fn: (node: Element) => unknown) =>
      fn({
        ownerDocument: { defaultView: { getComputedStyle: () => ({ color: "rgb(220, 38, 38)" }) } },
      } as unknown as Element)
    );
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({ locator: () => locator({ locator: () => locator({ evaluate }) }) })
          : locator(),
    });
    await expect(new WebNewDriver(page).assistantErrorTint(0)).resolves.toBe("rgb(220, 38, 38)");
    expect(evaluate).toHaveBeenCalledTimes(1);
  });

  it("reads the empty state from the thread body and null once rows render", async () => {
    const emptyPage = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-transcript"]') return locator({ count: async () => 0 });
        if (selector === '[data-testid="assistant-thread-body"]') {
          return locator({ innerText: async () => " Ask about your issues. " });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(emptyPage).assistantEmptyState()).resolves.toBe("Ask about your issues.");
    const filledPage = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]' ? locator({ count: async () => 1 }) : locator(),
    });
    await expect(new WebNewDriver(filledPage).assistantEmptyState()).resolves.toBeNull();
    await expect(
      new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) })).assistantEmptyState()
    ).resolves.toBeNull();
  });

  it("reads the inline error line only while it is shown", async () => {
    const hidden = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-error-line"]' ? locator({ isVisible: async () => false }) : locator(),
    });
    await expect(new WebNewDriver(hidden).assistantErrorLine()).resolves.toBeNull();
    const shown = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-error-line"]'
          ? locator({ innerText: async () => " turn_active " })
          : locator(),
    });
    await expect(new WebNewDriver(shown).assistantErrorLine()).resolves.toBe("turn_active");
    const empty = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-error-line"]' ? locator({ innerText: async () => "  " }) : locator(),
    });
    await expect(new WebNewDriver(empty).assistantErrorLine()).resolves.toBeNull();
  });

  it("reports the tail scroll from the thread body", async () => {
    const evaluate = vi.fn(async (fn: (node: Element) => unknown) =>
      fn({ scrollHeight: 500, scrollTop: 480, clientHeight: 20 } as unknown as Element)
    );
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-thread-body"]' ? locator({ evaluate }) : locator(),
    });
    await expect(new WebNewDriver(page).assistantIsScrolledToBottom()).resolves.toBe(true);
  });

  it("clicks the stop action by its label", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'button[aria-label="Stop response"]' ? locator({ click }) : locator(),
    });
    await new WebNewDriver(page).assistantClickStop();
    expect(click).toHaveBeenCalledTimes(1);
  });
});

describe("stream stubs", () => {
  it("serves the queued frames as chat.event blocks and records the URL", async () => {
    const route = vi.fn(async () => undefined);
    const page = pageOf({ route });
    const driver = new WebNewDriver(page);
    await driver.assistantStubStream("t1", [
      { seq: 4, kind: "assistant_delta", message: "m1", payload: { params: { delta: "Hi" } } },
    ]);
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    const stub = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(stub as unknown as Route);
    const options = stub.fulfill.mock.calls[0]?.[0] as { body: string };
    expect(options.body).toContain("event: chat.event");
    expect(options.body).toContain('"kind":"assistant_delta"');
    expect(options.body).toContain('"message":"m1"');
    await expect(driver.assistantStreamRequestUrls("t1")).resolves.toEqual([
      "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/",
    ]);
  });

  it("re-stubs without installing a second handler and holds an empty queue open", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubStream("t1", []);
    await driver.assistantStubStream("t1", []);
    expect(route).toHaveBeenCalledTimes(1);
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    const stub = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(stub as unknown as Route);
    const firstCall = stub.fulfill.mock.calls[0];
    expect(firstCall).toBeDefined();
    expect((firstCall![0] as { body: string }).body).toContain("keepalive");
  });

  it("aborts every request while the channel is blocked and stops after clearing", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantBlockStream("t1");
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    const blocked = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(blocked as unknown as Route);
    expect(blocked.abort).toHaveBeenCalledTimes(1);
    await driver.assistantClearStreamBlock("t1");
    const open = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(open as unknown as Route);
    expect(open.fulfill).toHaveBeenCalledTimes(1);
  });

  it("clears the stub, its handler and the observed URLs", async () => {
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ unroute }));
    await driver.assistantStubStream("t1", []);
    await driver.assistantClearStreamStub("t1");
    expect(unroute).toHaveBeenCalledWith("**/ai-assistant/threads/t1/events**");
    await expect(driver.assistantStreamRequestUrls("t1")).resolves.toEqual([]);
  });
});

describe("one-shot delay stubs", () => {
  it("delays only the first message POST and falls through afterwards", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantDelaySend(1);
    const handler = handlerFor(route, "**/api/workspaces/*/ai-assistant/threads/*/messages/");
    const post = stubRoute("POST", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/");
    const read = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/");
    await handler(post as unknown as Route);
    await handler(read as unknown as Route);
    await handler(post as unknown as Route);
    expect(post.fallback).toHaveBeenCalledTimes(2);
    expect(read.fallback).toHaveBeenCalledTimes(1);
    const unroute = vi.fn(async () => undefined);
    const clearing = new WebNewDriver(pageOf({ unroute }));
    await clearing.assistantClearSendDelay();
    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/ai-assistant/threads/*/messages/");
  });
});

function transcriptPage(rows: StubLocator[]): Page {
  return pageOf({
    locator: (selector: string) =>
      selector === '[data-testid="assistant-transcript"]'
        ? locator({
            locator: (inner: string) =>
              inner === ":scope > li"
                ? locator({ count: async () => rows.length, nth: (i) => rows[i] as never })
                : locator(),
          })
        : locator(),
  });
}

describe("transcript read fallbacks", () => {
  it("falls back to the assistant role when the role read fails", async () => {
    const page = transcriptPage([
      locator({
        getAttribute: async () => {
          throw new Error("detached");
        },
        innerText: async () => "Hello",
      }),
    ]);
    await expect(new WebNewDriver(page).assistantBubbles()).resolves.toEqual([{ role: "assistant", text: "Hello" }]);
  });

  it("skips a bubble whose text read fails", async () => {
    const page = transcriptPage([
      locator({
        getAttribute: async () => "assistant",
        innerText: async () => {
          throw new Error("detached");
        },
      }),
    ]);
    await expect(new WebNewDriver(page).assistantBubbles()).resolves.toEqual([]);
  });

  it("skips a bubble whose text read returns nothing", async () => {
    const page = transcriptPage([
      locator({
        getAttribute: async () => "assistant",
        innerText: async () => null as unknown as string,
      }),
    ]);
    await expect(new WebNewDriver(page).assistantBubbles()).resolves.toEqual([]);
  });

  it("reads an empty tint when the row has no document view", async () => {
    const evaluate = vi.fn(async (fn: (node: Element) => unknown) =>
      fn({ ownerDocument: { defaultView: null } } as unknown as Element)
    );
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-transcript"]'
          ? locator({ locator: () => locator({ locator: () => locator({ evaluate }) }) })
          : locator(),
    });
    await expect(new WebNewDriver(page).assistantErrorTint(0)).resolves.toBe("");
  });

  it("reports no empty state when the body text read fails", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-transcript"]') return locator({ count: async () => 0 });
        if (selector === '[data-testid="assistant-thread-body"]') {
          return locator({
            innerText: async () => {
              throw new Error("detached");
            },
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).assistantEmptyState()).resolves.toBeNull();
  });

  it("reports no empty state when the body holds only whitespace", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-transcript"]') return locator({ count: async () => 0 });
        if (selector === '[data-testid="assistant-thread-body"]') {
          return locator({ innerText: async () => "  " });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).assistantEmptyState()).resolves.toBeNull();
  });

  it("reports no empty state when the body text read returns nothing", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="assistant-transcript"]') return locator({ count: async () => 0 });
        if (selector === '[data-testid="assistant-thread-body"]') {
          return locator({ innerText: async () => null as unknown as string });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).assistantEmptyState()).resolves.toBeNull();
  });

  it("reports not-at-bottom when the thread body is absent", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-thread-body"]' ? locator({ count: async () => 0 }) : locator(),
    });
    await expect(new WebNewDriver(page).assistantIsScrolledToBottom()).resolves.toBe(false);
  });
});

describe("stream stub fallbacks", () => {
  it("serves keepalive to a request racing the stub teardown", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubStream("t1", [
      { seq: 4, kind: "assistant_delta", message: "m1", payload: { params: { delta: "Hi" } } },
    ]);
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    await driver.assistantClearStreamStub("t1");
    const stub = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(stub as unknown as Route);
    expect(stub.abort).not.toHaveBeenCalled();
    const options = stub.fulfill.mock.calls[0]?.[0] as { body: string };
    expect(options.body).toContain("keepalive");
  });

  it("serves a null message for turn-level frames", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubStream("t1", [{ seq: 1, kind: "turn_started", payload: { turn_id: "t" } }]);
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    const stub = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(stub as unknown as Route);
    const options = stub.fulfill.mock.calls[0]?.[0] as { body: string };
    expect(options.body).toContain('"message":null');
  });

  it("reports no stream URLs for a thread that was never stubbed", async () => {
    const driver = new WebNewDriver(pageOf({}));
    await expect(driver.assistantStreamRequestUrls("never")).resolves.toEqual([]);
  });

  it("keeps the queued frames when blocking after a stub", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubStream("t1", [
      { seq: 4, kind: "assistant_delta", message: "m1", payload: { params: { delta: "Hi" } } },
    ]);
    await driver.assistantBlockStream("t1");
    const handler = handlerFor(route, "**/ai-assistant/threads/t1/events**");
    const blocked = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(blocked as unknown as Route);
    expect(blocked.abort).toHaveBeenCalledTimes(1);
    await driver.assistantClearStreamBlock("t1");
    const open = stubRoute("GET", "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/");
    await handler(open as unknown as Route);
    const options = open.fulfill.mock.calls[0]?.[0] as { body: string };
    expect(options.body).toContain('"kind":"assistant_delta"');
  });
});
