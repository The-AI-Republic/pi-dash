// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the oracle driver's refused-description-PATCH hook
// (NEWFRONT-359, PAGE-064). A Page double proves the route matcher refuses
// only body PATCHes and leaves the fallback's binary GET and record writes
// alone; no browser or parity stack is involved.
import type { Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebDriver } from "./web";

interface RouteRegistration {
  pattern: string | RegExp;
  handler: (route: Route) => Promise<void>;
}

function pageDouble(): {
  page: Page;
  route: ReturnType<typeof vi.fn>;
  routes: RouteRegistration[];
} {
  const routes: RouteRegistration[] = [];
  const route = vi.fn(async (pattern: string | RegExp, handler: (route: Route) => Promise<void>) => {
    routes.push({ pattern, handler });
  });
  return { page: { route } as unknown as Page, route, routes };
}

function routeDouble(method: string): {
  fulfill: ReturnType<typeof vi.fn>;
  cont: ReturnType<typeof vi.fn>;
  route: Route;
} {
  const fulfill = vi.fn(async () => undefined);
  const cont = vi.fn(async () => undefined);
  const route = { request: () => ({ method: () => method }), fulfill, continue: cont } as unknown as Route;
  return { fulfill, cont, route };
}

const descriptionUrl = "https://api.test/api/workspaces/acme/projects/p1/pages/pg-1/description/";

describe("pagesHeaderFailDescriptionWrites", () => {
  it("installs one route that refuses only description patches", async () => {
    const stub = pageDouble();
    const driver = new WebDriver(stub.page);

    await driver.pagesHeaderFailDescriptionWrites();
    await driver.pagesHeaderFailDescriptionWrites();

    expect(stub.route).toHaveBeenCalledTimes(1);
    const registration = stub.routes[0] as RouteRegistration;
    expect(registration.pattern).toBeInstanceOf(RegExp);
    const pattern = registration.pattern as RegExp;
    expect(pattern.test(descriptionUrl)).toBe(true);
    expect(pattern.test("https://api.test/api/workspaces/acme/projects/p1/pages/pg-1/")).toBe(false);

    const refused = routeDouble("PATCH");
    await registration.handler(refused.route);
    expect(refused.fulfill).toHaveBeenCalledWith({ status: 500, contentType: "application/json", body: "{}" });
    expect(refused.cont).not.toHaveBeenCalled();

    const read = routeDouble("GET");
    await registration.handler(read.route);
    expect(read.cont).toHaveBeenCalledTimes(1);
    expect(read.fulfill).not.toHaveBeenCalled();

    await driver.pagesHeaderUnfailDescriptionWrites();
    const passed = routeDouble("PATCH");
    await registration.handler(passed.route);
    expect(passed.cont).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });
});
