// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the cross-edition sameness driver additions
// (NEWFRONT-819, CMT-025). The scenario's last step is only meaningful if
// the spy attributes requests to the right capability and the capability
// reads answer presence honestly, so a stub Page replays recorded requests
// here instead of a browser.
import type { Locator, Page, Request, Route } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebDriver } from "./web";

interface StubLocator {
  count?: () => Promise<number>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  locator?: (selector: string) => StubLocator;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    waitFor: async () => undefined,
    ...overrides,
  };
  if (node.first === undefined) node.first = () => node;
  if (node.nth === undefined) node.nth = () => node;
  if (node.locator === undefined) node.locator = () => node;
  if (node.getByRole === undefined) node.getByRole = () => node;
  return node as unknown as Locator;
}

interface SpyPage {
  page: Page;
  handler: (route: Route) => Promise<void>;
  continued: number;
  routes: string[];
  unrouted: string[];
  reloads: number;
  /** Nodes the "Activity" heading resolves to; one per `locator` call path. */
  activityHeading: Locator;
}

/**
 * A stub page: `getByText("Activity")` yields the heading, routing records
 * the installed handler, and reload waits resolve immediately.
 */
function spyPage(parts: { heading?: Locator; commentCard?: Locator; trigger?: Locator } = {}): SpyPage {
  const state = {
    handler: async () => undefined,
    continued: 0,
    routes: [] as string[],
    unrouted: [] as string[],
    reloads: 0,
    activityHeading: parts.heading ?? locator(),
    page: undefined as unknown as Page,
  };
  const heading = state.activityHeading;
  const section = locator({ locator: () => locator() });
  heading.locator = () => section;
  const page = {
    getByText: () => heading,
    route: async (pattern: string, handler: (route: Route) => Promise<void>) => {
      state.routes.push(pattern);
      state.handler = handler;
    },
    unroute: async (pattern: string) => {
      state.unrouted.push(pattern);
    },
    reload: async () => {
      state.reloads += 1;
      return null;
    },
    locator: (selector: string) => {
      if (selector.startsWith('div[id^="comment-"]') || selector === 'button[aria-haspopup="dialog"]') {
        return parts.commentCard ?? locator();
      }
      return locator();
    },
    evaluate: async () => (parts.trigger === undefined ? -1 : 0),
  };
  state.page = page as unknown as Page;
  return state as unknown as SpyPage;
}

function request(method: string, url: string): Request {
  return { method: () => method, url: () => url } as unknown as Request;
}

/** Feed one recorded request through the installed spy handler. */
async function observe(spy: SpyPage, method: string, path: string): Promise<void> {
  let continued = false;
  await spy.handler({
    request: () => request(method, `http://localhost:13020${path}`),
    continue: async () => {
      continued = true;
    },
  } as unknown as Route);
  if (continued) spy.continued += 1;
}

describe("activityBeginTrafficSpy", () => {
  it("installs the route, reloads the detail and resets the counters", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await observe(spy, "GET", "/api/workspaces/acme/projects/p/issues/i/history/");
    await driver.activityBeginTrafficSpy();
    expect(spy.routes).toEqual(["**/api/**"]);
    expect(spy.reloads).toBe(1);
    expect(await driver.activityTrafficCounts()).toEqual({
      historyReads: 0,
      commentReads: 0,
      commentCreates: 0,
      mentionSearches: 0,
      commentReactionCreates: 0,
      commentReactionDeletes: 0,
      issueReactionCreates: 0,
      issueReactionDeletes: 0,
      unexpectedWrites: 0,
    });
  });
});

describe("activityTrafficCounts", () => {
  it("attributes history and comment reads, and passes them through", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy();
    await observe(spy, "GET", "/api/workspaces/acme/projects/p/issues/i/history/?activity_type=issue-comment");
    await observe(spy, "GET", "/api/workspaces/acme/projects/p/issues/i/history/?activity_type=issue-property");
    await observe(spy, "GET", "/api/workspaces/acme/projects/p/issues/i/comments/");
    expect(spy.continued).toBe(3);
    expect(await driver.activityTrafficCounts()).toMatchObject({ historyReads: 2, commentReads: 1 });
  });

  it("attributes comment creates, mention searches and both reaction families", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy();
    await observe(spy, "POST", "/api/workspaces/acme/projects/p/issues/i/comments/");
    await observe(spy, "GET", "/api/workspaces/acme/entity-search/?query=Pa&query_type=user_mention");
    await observe(spy, "POST", "/api/workspaces/acme/projects/p/comments/c1/reactions/");
    await observe(spy, "DELETE", "/api/workspaces/acme/projects/p/comments/c1/reactions/128077/");
    await observe(spy, "POST", "/api/workspaces/acme/projects/p/issues/i/reactions/");
    await observe(spy, "DELETE", "/api/workspaces/acme/projects/p/issues/i/reactions/128077/");
    expect(await driver.activityTrafficCounts()).toEqual({
      historyReads: 0,
      commentReads: 0,
      commentCreates: 1,
      mentionSearches: 1,
      commentReactionCreates: 1,
      commentReactionDeletes: 1,
      issueReactionCreates: 1,
      issueReactionDeletes: 1,
      unexpectedWrites: 0,
    });
  });

  it("counts any write outside the comment and reaction endpoints as unexpected", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy();
    await observe(spy, "PATCH", "/api/workspaces/acme/projects/p/issues/i/");
    await observe(spy, "POST", "/api/workspaces/acme/projects/p/issues/i/links/");
    // Reads never count as an unexpected write.
    await observe(spy, "GET", "/api/workspaces/acme/projects/p/issues/i/comments/");
    const counts = await driver.activityTrafficCounts();
    expect(counts.unexpectedWrites).toBe(2);
    expect(counts.commentReads).toBe(1);
  });

  it("replaces an earlier spy instead of stacking one", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy();
    await driver.activityBeginTrafficSpy();
    expect(spy.routes).toEqual(["**/api/**", "**/api/**"]);
    expect(spy.unrouted).toEqual(["**/api/**"]);
    expect(spy.reloads).toBe(2);
  });

  it("keeps the page in place when the caller only wants a clean counter", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy({ reload: false });
    expect(spy.routes).toEqual(["**/api/**"]);
    expect(spy.reloads).toBe(0);
    expect(await driver.activityTrafficCounts()).toMatchObject({ historyReads: 0, unexpectedWrites: 0 });
  });

  it("flags an unsupported method on a reaction endpoint instead of counting it as a capability", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy({ reload: false });
    await observe(spy, "PUT", "/api/workspaces/acme/projects/p/comments/c1/reactions/");
    await observe(spy, "PATCH", "/api/workspaces/acme/projects/p/comments/c1/reactions/128077/");
    await observe(spy, "PUT", "/api/workspaces/acme/projects/p/issues/i/reactions/");
    await observe(spy, "PATCH", "/api/workspaces/acme/projects/p/issues/i/reactions/128077/");
    const counts = await driver.activityTrafficCounts();
    expect(counts.unexpectedWrites).toBe(4);
    expect(counts.commentReactionCreates).toBe(0);
    expect(counts.commentReactionDeletes).toBe(0);
    expect(counts.issueReactionCreates).toBe(0);
    expect(counts.issueReactionDeletes).toBe(0);
  });

  it("leaves HEAD and OPTIONS requests out of the write count", async () => {
    const spy = spyPage();
    const driver = new WebDriver(spy.page);
    await driver.activityBeginTrafficSpy({ reload: false });
    await observe(spy, "HEAD", "/api/workspaces/acme/projects/p/issues/i/");
    await observe(spy, "OPTIONS", "/api/workspaces/acme/projects/p/issues/i/");
    expect((await driver.activityTrafficCounts()).unexpectedWrites).toBe(0);
  });
});

describe("reaction control presence", () => {
  it("reports the per-card picker trigger when the card carries it", async () => {
    const spy = spyPage({ commentCard: locator({ count: async () => 1 }) });
    const card = locator({ locator: () => locator({ count: async () => 1 }) });
    spy.page.locator = ((selector: string) => {
      if (selector.startsWith('div[id^="comment-"]')) {
        return locator({ count: async () => 1, first: () => card });
      }
      return locator();
    }) as Page["locator"];
    await expect(new WebDriver(spy.page).activityCommentReactionControlVisible("hello")).resolves.toBe(true);
  });

  it("reports no per-card control when the card is missing", async () => {
    const spy = spyPage();
    spy.page.locator = (() =>
      locator({ count: async () => 0, first: () => locator({ count: async () => 0 }) })) as Page["locator"];
    await expect(new WebDriver(spy.page).activityCommentReactionControlVisible("hello")).resolves.toBe(false);
  });

  it("reports the issue reaction control absent when no trigger sits above the feed", async () => {
    const spy = spyPage();
    await expect(new WebDriver(spy.page).issueReactionControlVisible()).resolves.toBe(false);
  });

  it("reports the issue reaction control present when a trigger sits above the feed", async () => {
    const spy = spyPage({ trigger: locator() });
    await expect(new WebDriver(spy.page).issueReactionControlVisible()).resolves.toBe(true);
  });
});

describe("composerEditAppend", () => {
  it("appends to the oracle edit draft without selecting it first", async () => {
    const steps: string[] = [];
    const spy = spyPage({
      commentCard: locator({
        click: async () => void steps.push("click"),
        press: async (key: string) => void steps.push(`press:${key}`),
        pressSequentially: async (value: string) => void steps.push(`type:${value}`),
      }),
    });
    await new WebDriver(spy.page).composerEditAppend(" kept");
    expect(steps).toEqual(["click", "press:Home", "press:End", "type: kept"]);
  });
});
