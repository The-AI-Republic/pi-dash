// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver loader/freshness methods (NEWFRONT-658,
// ISS-004–006). A stub Page records the loader hooks and the stall routes
// each method installs, so dropping them fails here before the parity stack.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  waitFor?: (options?: unknown) => Promise<void>;
  locator?: (selector: string) => Locator;
  first?: () => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    isVisible: async () => true,
    waitFor: async () => undefined,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node(),
    first: () => self as unknown as Locator,
  };
  for (const [name, value] of Object.entries(chain)) {
    if (self[name as keyof StubNode] === undefined) {
      (self as Record<string, unknown>)[name] = value;
    }
  }
  return self as unknown as Locator;
}

type RouteHandler = (route: {
  request: () => { method: () => string };
  continue: () => Promise<void>;
}) => Promise<void>;

interface PageRoutes {
  locator?: (selector: string) => Locator;
  testId?: (id: string) => Locator;
  route?: (pattern: unknown, handler: RouteHandler) => Promise<void>;
  unrouteAll?: (options: unknown) => Promise<void>;
}

interface PageStub {
  page: Page;
  selectors: string[];
  testIds: string[];
  routes: Array<{ pattern: unknown; handler: RouteHandler }>;
  unroutes: unknown[];
}

function pageOf(routes: PageRoutes = {}): PageStub {
  const state: PageStub = {
    page: undefined as unknown as Page,
    selectors: [],
    testIds: [],
    routes: [],
    unroutes: [],
  };
  state.page = {
    locator: (selector: string) => {
      state.selectors.push(selector);
      return routes.locator?.(selector) ?? node({ count: async () => 0 });
    },
    getByTestId: (id: string) => {
      state.testIds.push(id);
      return routes.testId?.(id) ?? node({ count: async () => 0 });
    },
    route: async (pattern: unknown, handler: RouteHandler) => {
      state.routes.push({ pattern, handler });
      await routes.route?.(pattern, handler);
    },
    unrouteAll: async (options: unknown) => {
      state.unroutes.push(options);
      await routes.unrouteAll?.(options);
    },
  } as unknown as Page;
  return state;
}

function fakeRoute(method: string, continued: () => void): Parameters<RouteHandler>[0] {
  return {
    request: () => ({ method: () => method }),
    continue: async () => void continued(),
  };
}

describe("web-new loader visibility", () => {
  it("reads either skeleton surface", async () => {
    const loading = pageOf({
      testId: (id: string) => (id === "list-loading" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(loading.page).layoutsSkeletonVisible()).resolves.toBe(true);
    const section = pageOf({
      testId: (id: string) => (id === "section-loading" ? node() : node({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(section.page).layoutsSkeletonVisible()).resolves.toBe(true);
    const settled = pageOf();
    await expect(new WebNewDriver(settled.page).layoutsSkeletonVisible()).resolves.toBe(false);
    expect(loading.testIds).toEqual(["list-loading"]);
    expect(section.testIds).toEqual(["list-loading", "section-loading"]);
  });

  it("reads the floating mutation spinner", async () => {
    const shown = pageOf({ testId: () => node() });
    await expect(new WebNewDriver(shown.page).layoutsMutationSpinnerVisible()).resolves.toBe(true);
    const hidden = pageOf();
    await expect(new WebNewDriver(hidden.page).layoutsMutationSpinnerVisible()).resolves.toBe(false);
    expect(shown.testIds).toEqual(["issues-mutation-spinner"]);
  });

  it("reads the unconfirmed temp row", async () => {
    const pending = pageOf({ locator: () => node() });
    await expect(new WebNewDriver(pending.page).layoutsTempRowVisible()).resolves.toBe(true);
    const confirmed = pageOf();
    await expect(new WebNewDriver(confirmed.page).layoutsTempRowVisible()).resolves.toBe(false);
    const hiding = pageOf({ locator: () => node({ isVisible: async () => false }) });
    await expect(new WebNewDriver(hiding.page).layoutsTempRowVisible()).resolves.toBe(false);
    expect(pending.selectors).toEqual(['article[data-temprow="true"]']);
  });
});

describe("web-new request stalls", () => {
  it("stalls issue GETs and passes other traffic through", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.layoutsStallIssuesGet(20);
    expect(stub.routes).toHaveLength(1);
    const { pattern, handler } = stub.routes[0] ?? { pattern: null, handler: null };
    expect(String(pattern)).toBe(String(/\/issues\//));
    expect(pattern instanceof RegExp && pattern.test("/api/workspaces/w/projects/p/issues/")).toBe(true);
    expect(pattern instanceof RegExp && pattern.test("/api/workspaces/w/projects/")).toBe(false);
    if (handler === null) throw new Error("no stall handler installed");
    let continued = 0;
    const started = Date.now();
    await handler(fakeRoute("GET", () => void (continued += 1)));
    expect(Date.now() - started).toBeGreaterThanOrEqual(20);
    await handler(fakeRoute("POST", () => void (continued += 1)));
    expect(continued).toBe(2);
  });

  it("counts stalled mutations and resets on re-arm", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.layoutsStallIssueMutation(0);
    const first = stub.routes[0]?.handler;
    if (first === undefined) throw new Error("no stall handler installed");
    let continued = 0;
    const fire = (method: string): Promise<void> => first(fakeRoute(method, () => void (continued += 1)));
    await fire("POST");
    await fire("PATCH");
    await fire("PUT");
    await fire("GET");
    await expect(driver.layoutsStalledMutationCount()).resolves.toBe(3);
    expect(continued).toBe(4);
    await driver.layoutsStallIssueMutation(0);
    await expect(driver.layoutsStalledMutationCount()).resolves.toBe(0);
  });

  it("releases stalls by unrouting everything", async () => {
    const stub = pageOf();
    await new WebNewDriver(stub.page).layoutsReleaseStalls();
    expect(stub.unroutes).toEqual([{ behavior: "wait" }]);
  });
});
