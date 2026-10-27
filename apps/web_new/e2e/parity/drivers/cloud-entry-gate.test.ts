// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the cloud entry gate parity helpers (NEWFRONT-496,
// ADM-076/077/079): the auth-probe fault release the probe-blip
// scenario leans on, and the history read that proves the OIDC handoff
// replaced the gate's entry rather than pushing a back-button loop. A
// stub Page stands in for Playwright; no browser is involved.
import type { Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";
import { WebDriver } from "./web";

interface GateRoute {
  matches: (url: URL) => boolean;
  handler: (route: unknown) => Promise<void>;
}

interface StubPage {
  page: Page;
  routes: GateRoute[];
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): StubPage {
  const routes: GateRoute[] = [];
  const page = {
    route: vi.fn(async (pattern: (url: URL) => boolean, handler: (route: unknown) => Promise<void>) => {
      routes.push({ matches: pattern, handler });
    }),
    goBack: vi.fn(async () => null),
    url: () => "https://admin.test/god-mode/",
    ...overrides,
  } as unknown as Page;
  return { page, routes };
}

/** The installed probe route, found by the URL it is meant to match. */
function probeRouteOf(stub: StubPage): GateRoute {
  const probe = stub.routes.find((route) => route.matches(new URL("https://api.test/api/auth/me/")));
  expect(probe).toBeDefined();
  if (probe === undefined) throw new Error("no probe route installed");
  return probe;
}

/** One GET read of the probe, reporting how the route answered it. */
async function readProbe(route: GateRoute): Promise<"fulfilled" | "continued"> {
  const fulfill = vi.fn(async () => undefined);
  const proceed = vi.fn(async () => undefined);
  await route.handler({
    request: () => ({ method: () => "GET" }),
    fulfill,
    continue: proceed,
  } as unknown as Route);
  if (fulfill.mock.calls.length > 0) return "fulfilled";
  expect(proceed).toHaveBeenCalledTimes(1);
  return "continued";
}

describe("WebNewDriver cloud gate helpers", () => {
  it("holds a probe fault until it is explicitly released", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.adminCloudFailAuthProbe(Number.POSITIVE_INFINITY, 503);
    const probe = probeRouteOf(stub);
    // Infinity - 1 is still Infinity, so the outage outlasts any
    // StrictMode double read the scenario runs into.
    await expect(readProbe(probe)).resolves.toBe("fulfilled");
    await expect(readProbe(probe)).resolves.toBe("fulfilled");
    expect(await driver.adminCloudGateFetchCounts()).toEqual({ adminCheck: 0, probe: 2 });
    await driver.adminCloudReleaseAuthProbeFault();
    await expect(readProbe(probe)).resolves.toBe("continued");
  });

  it("leaves reads alone before any probe fault is armed", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.adminCloudStartGateFetchSpy();
    await expect(readProbe(probeRouteOf(stub))).resolves.toBe("continued");
  });

  it("reports where the tab lands after going back", async () => {
    const stub = pageOf({ url: () => "about:blank" });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.adminCloudGoBack()).resolves.toBe("about:blank");
    expect(stub.page.goBack).toHaveBeenCalledTimes(1);
  });

  it("survives a go-back with no earlier entry and reads the settled URL", async () => {
    const stub = pageOf({
      goBack: vi.fn(async () => {
        throw new Error("no history entry");
      }),
      url: () => "https://admin.test/god-mode/",
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.adminCloudGoBack()).resolves.toBe("https://admin.test/god-mode/");
  });
});

describe("WebDriver cloud gate helpers", () => {
  it("holds a probe fault until it is explicitly released", async () => {
    const stub = pageOf();
    const driver = new WebDriver(stub.page);
    await driver.adminCloudFailAuthProbe(Number.POSITIVE_INFINITY, 503);
    const probe = probeRouteOf(stub);
    await expect(readProbe(probe)).resolves.toBe("fulfilled");
    await driver.adminCloudReleaseAuthProbeFault();
    await expect(readProbe(probe)).resolves.toBe("continued");
  });

  it("reports where the tab lands after going back", async () => {
    const stub = pageOf({ url: () => "https://oracle.test/" });
    const driver = new WebDriver(stub.page);
    await expect(driver.adminCloudGoBack()).resolves.toBe("https://oracle.test/");
  });
});
