// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cross-cutting singles (NEWFRONT-658,
// ISS-220–225): the intake-issue open and triage chrome, the archived
// detail open, the global-view read, and the one-shot issue PATCH
// failure. A stub Page stands in for Playwright.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  waitFor?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByText?: (text: string | RegExp, options?: unknown) => Locator;
  first?: () => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    waitFor: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    getByText: () => self,
    first: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

type RouteHandler = (route: Route) => Promise<void>;

interface PageParts {
  goto?: (url: string, options?: unknown) => Promise<void>;
  waitForLoadState?: (state: string) => Promise<void>;
  url?: () => string;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByText?: (text: string | RegExp, options?: unknown) => Locator;
  route?: (matches: (url: URL) => boolean, handler: RouteHandler) => Promise<void>;
  unroute?: (matches: (url: URL) => boolean, handler: RouteHandler) => Promise<void>;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    goto: parts.goto ?? (async () => undefined),
    waitForLoadState: parts.waitForLoadState ?? (async () => undefined),
    url: parts.url ?? (() => "https://parity.test/"),
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    getByText: (text: string | RegExp, options?: unknown) => parts.getByText?.(text, options) ?? locator(),
    route: parts.route ?? (async () => undefined),
    unroute: parts.unroute ?? (async () => undefined),
  } as unknown as Page;
}

function fakeRoute(method: string, calls: string[]): Route {
  return {
    request: () => ({ method: () => method }),
    continue: async () => void calls.push("continued"),
    fulfill: async () => void calls.push("fulfilled"),
  } as unknown as Route;
}

describe("cross-cutting singles", () => {
  it("opens the intake row and waits for its feed", async () => {
    const gotos: string[] = [];
    const page = pageOf({ goto: async (url: string) => void gotos.push(url) });
    await new WebNewDriver(page).rulesOpenIntakeIssue("ws", "pid", "inbox1");
    expect(gotos).toEqual(["/ws/projects/pid/intake?inboxIssueId=inbox1"]);
  });

  it("reads the triage chrome only when both controls show", async () => {
    const both = pageOf();
    await expect(new WebNewDriver(both).rulesIntakeTriageVisible()).resolves.toBe(true);
    const oneHidden = pageOf({
      getByTestId: (id: string | RegExp) =>
        String(id) === "intake-triage-decline" ? locator({ isVisible: async () => false }) : locator(),
    });
    await expect(new WebNewDriver(oneHidden).rulesIntakeTriageVisible()).resolves.toBe(false);
  });

  it("opens the archived detail on the archives URL", async () => {
    const gotos: string[] = [];
    const page = pageOf({
      goto: async (url: string) => void gotos.push(url),
      getByText: () => locator({ count: async () => 0 }),
    });
    await new WebNewDriver(page).openArchivedIssueDetail("ws", "pid", "iid");
    expect(gotos).toEqual(["/ws/projects/pid/archives/issues/iid"]);
  });

  it("refuses an archived detail that renders an error state", async () => {
    const page = pageOf({ getByText: () => locator({ count: async () => 1 }) });
    await expect(new WebNewDriver(page).openArchivedIssueDetail("ws", "pid", "iid")).rejects.toThrow(
      "rendered an error state"
    );
  });

  it("reads the global view by exact text", async () => {
    const seen = pageOf({ getByText: () => locator({ count: async () => 2 }) });
    await expect(new WebNewDriver(seen).globalViewIssueVisible("Alpha")).resolves.toBe(true);
    const bare = pageOf({ getByText: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(bare).globalViewIssueVisible("Alpha")).resolves.toBe(false);
  });

  it("routes only issue PATCH traffic to the one-shot failure", async () => {
    const routed: Array<{ matches: (url: URL) => boolean; handler: RouteHandler }> = [];
    const unrouted: Array<{ matches: (url: URL) => boolean; handler: RouteHandler }> = [];
    const page = pageOf({
      route: async (matches, handler) => void routed.push({ matches, handler }),
      unroute: async (matches, handler) => void unrouted.push({ matches, handler }),
    });
    await new WebNewDriver(page).failNextIssuePatch(500, 0);
    expect(routed).toHaveLength(1);
    const { matches, handler } = routed[0]!;
    const issue = new URL("https://parity.test/api/workspaces/ws/projects/pid/issues/iid/");
    const comments = new URL("https://parity.test/api/workspaces/ws/projects/pid/issues/iid/comments/");
    const labels = new URL("https://parity.test/api/workspaces/ws/projects/pid/issue-labels/");
    expect(matches(issue)).toBe(true);
    expect(matches(comments)).toBe(true);
    expect(matches(labels)).toBe(false);

    const reads: string[] = [];
    await handler(fakeRoute("GET", reads));
    expect(reads).toEqual(["continued"]);
    expect(unrouted).toHaveLength(0);

    const writes: string[] = [];
    await handler(fakeRoute("PATCH", writes));
    expect(writes).toEqual(["fulfilled"]);
    expect(unrouted).toHaveLength(1);
  });

  it("clears an unarmed failure route quietly", async () => {
    let unroutes = 0;
    const page = pageOf({ unroute: async () => void (unroutes += 1) });
    await new WebNewDriver(page).clearIssuePatchFailure();
    expect(unroutes).toBe(0);
  });

  it("opens the intake picker once, then leaves an open popup alone", async () => {
    let clicks = 0;
    const closed = pageOf({
      getByRole: (role: string) =>
        role === "listbox"
          ? locator({ count: async () => 0 })
          : locator({ getByRole: () => locator({ click: async () => void (clicks += 1) }) }),
    });
    await new WebNewDriver(closed).intakeStateOpenPicker();
    expect(clicks).toBe(1);

    const open = pageOf({
      getByRole: (role: string) =>
        role === "listbox"
          ? locator({ count: async () => 1 })
          : locator({ getByRole: () => locator({ click: async () => void (clicks += 1) }) }),
    });
    await new WebNewDriver(open).intakeStateOpenPicker();
    expect(clicks).toBe(1);
  });
});
