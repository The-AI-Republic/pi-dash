// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cloud session methods (NEWFRONT-653,
// CLOUD-010/015-020). A stub Page stands in for Playwright: the assertions
// cover the dialog capture and popup read, the refresh counter, the
// console tap, the tauri stub, the cookie helpers, and the provider button
// reads — never a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  innerText?: () => Promise<string>;
  first?: () => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 0,
    click: async () => undefined,
    waitFor: async () => undefined,
    innerText: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

interface StubPage {
  handlers: Map<string, (...args: never[]) => unknown>;
  routes: Map<string, (route: unknown) => unknown>;
  page: Page;
  on: ReturnType<typeof vi.fn>;
  off: ReturnType<typeof vi.fn>;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): StubPage {
  const handlers = new Map<string, (...args: never[]) => unknown>();
  const routes = new Map<string, (route: unknown) => unknown>();
  const on = vi.fn((event: string, handler: (...args: never[]) => unknown) => {
    handlers.set(event, handler);
  });
  const off = vi.fn((event: string) => {
    handlers.delete(event);
  });
  const page = {
    on,
    off,
    route: vi.fn(async (pattern: string, handler: (route: unknown) => unknown) => {
      routes.set(pattern, handler);
    }),
    unroute: vi.fn(async () => undefined),
    addInitScript: vi.fn(async () => undefined),
    waitForEvent: vi.fn(async () => null),
    waitForTimeout: vi.fn(async () => undefined),
    getByRole: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    context: vi.fn(() => ({ clearCookies: vi.fn(async () => undefined), cookies: vi.fn(async () => []) })),
    ...overrides,
  } as unknown as Page;
  return { handlers, routes, page, on, off };
}

function dialogOf(message: string) {
  return {
    message: () => message,
    accept: vi.fn(async () => undefined),
    dismiss: vi.fn(async () => undefined),
  };
}

describe("cloud paywall driver", () => {
  it("accepts the armed confirm and records its message", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudArmConfirm("accept");
    const dialog = dialogOf("You have reached the free creation limit.");
    await stub.handlers.get("dialog")?.(dialog as never);
    expect(dialog.accept).toHaveBeenCalledTimes(1);
    expect(dialog.dismiss).not.toHaveBeenCalled();
    expect(await driver.cloudConfirmMessages()).toEqual(["You have reached the free creation limit."]);
  });

  it("dismisses when armed to dismiss and resets on re-arm", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudArmConfirm("dismiss");
    const dialog = dialogOf("limit");
    await stub.handlers.get("dialog")?.(dialog as never);
    expect(dialog.dismiss).toHaveBeenCalledTimes(1);
    expect(dialog.accept).not.toHaveBeenCalled();
    await driver.cloudArmConfirm("accept");
    expect(await driver.cloudConfirmMessages()).toEqual([]);
    expect(stub.off).toHaveBeenCalledWith("dialog", expect.any(Function));
    expect(stub.on).toHaveBeenCalledWith("dialog", expect.any(Function));
  });

  it("reads the upgrade popup URL and closes it", async () => {
    const popup = { url: () => "https://accounts.test/upgrade", close: vi.fn(async () => undefined) };
    const stub = pageOf({ waitForEvent: vi.fn(async () => popup) });
    const driver = new WebNewDriver(stub.page);
    await driver.cloudArmConfirm("accept");
    await expect(driver.cloudUpgradePopupUrl()).resolves.toBe("https://accounts.test/upgrade");
    expect(popup.close).toHaveBeenCalledTimes(1);
  });

  it("refuses the popup read before arming or without a popup", async () => {
    const driver = new WebNewDriver(pageOf().page);
    await expect(driver.cloudUpgradePopupUrl()).rejects.toThrow("cloudArmConfirm must run");
    const stub = pageOf({ waitForEvent: vi.fn(async () => null) });
    const armed = new WebNewDriver(stub.page);
    await armed.cloudArmConfirm("accept");
    await expect(armed.cloudUpgradePopupUrl()).rejects.toThrow("no upgrade popup");
  });
});

describe("cloud session driver", () => {
  it("counts only the rotation requests and resets on re-arm", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudArmRefreshCounter();
    const seen = stub.handlers.get("request");
    seen?.({ url: () => "https://api.test/api/auth/refresh/" } as never);
    seen?.({ url: () => "https://api.test/api/users/me/" } as never);
    seen?.({ url: () => "https://api.test/api/auth/refresh/" } as never);
    expect(await driver.cloudRefreshHits()).toBe(2);
    await driver.cloudArmRefreshCounter();
    expect(await driver.cloudRefreshHits()).toBe(0);
  });

  it("fails the rotation route on demand", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudFailRefresh();
    const unroute = stub.page.unroute as unknown as ReturnType<typeof vi.fn>;
    expect(unroute).toHaveBeenCalledWith("**/api/auth/refresh/");
    const abort = vi.fn(async () => undefined);
    await stub.routes.get("**/api/auth/refresh/")?.({ abort });
    expect(abort).toHaveBeenCalledWith("failed");
  });

  it("records only console errors and resets on re-arm", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudArmConsoleErrors();
    const seen = stub.handlers.get("console");
    seen?.({ type: () => "error", text: () => "boom" } as never);
    seen?.({ type: () => "log", text: () => "hi" } as never);
    expect(await driver.cloudConsoleErrors()).toEqual(["boom"]);
    await driver.cloudArmConsoleErrors();
    expect(await driver.cloudConsoleErrors()).toEqual([]);
  });

  it("installs a tauri stub whose browser open is blocked", async () => {
    const stub = pageOf();
    const driver = new WebNewDriver(stub.page);
    await driver.cloudStubTauri();
    const addInitScript = stub.page.addInitScript as unknown as ReturnType<typeof vi.fn>;
    expect(addInitScript).toHaveBeenCalledTimes(1);
    const script = addInitScript.mock.calls[0]?.[0] as () => void;
    const fakeWindow = {} as Record<string, unknown>;
    vi.stubGlobal("window", fakeWindow);
    try {
      script();
      const tauri = fakeWindow["__TAURI__"] as { core: { invoke: () => Promise<unknown> } };
      await expect(tauri.core.invoke()).rejects.toThrow("blocked");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("expires the access cookie and reads the cookie jar", async () => {
    const clearCookies = vi.fn(async () => undefined);
    const cookies = vi.fn(async () => [
      { name: "pidash_refresh" },
      { name: "pidash_access" },
      { name: "pidash_access" },
    ]);
    const stub = pageOf({ context: vi.fn(() => ({ clearCookies, cookies })) });
    const driver = new WebNewDriver(stub.page);
    await driver.cloudExpireAccessCookie();
    expect(clearCookies).toHaveBeenCalledWith({ name: "pidash_access" });
    expect(await driver.cloudCookieNames()).toEqual(["pidash_access", "pidash_refresh"]);
  });

  it("counts the profile password fields", async () => {
    const count = vi.fn(async () => 0);
    const stub = pageOf({ locator: vi.fn(() => locator({ count })) });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.cloudProfilePasswordFieldCount()).resolves.toBe(0);
    expect(stub.page.locator).toHaveBeenCalledWith('input[type="password"]');
    expect(count).toHaveBeenCalledTimes(1);
  });
});

describe("cloud provider button driver", () => {
  it("reads the provider button beside the standard continue", async () => {
    const provider = locator({ innerText: async () => "  Sign up with AI Republic  " });
    const standard = locator({ count: async () => 1 });
    const getByRole = vi.fn((role: string, options?: { name?: unknown }) =>
      String((options?.name as RegExp | undefined)?.source ?? "").includes("ai republic") ? provider : standard
    );
    const stub = pageOf({ getByRole });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.cloudProviderButtonFacts()).resolves.toEqual({
      visible: true,
      label: "Sign up with AI Republic",
      standardContinueVisible: true,
    });
  });

  it("reports a missing provider button without a label", async () => {
    const provider = locator({
      waitFor: async () => {
        throw new Error("timeout");
      },
    });
    const standard = locator({ count: async () => 1 });
    const getByRole = vi.fn((role: string, options?: { name?: unknown }) =>
      String((options?.name as RegExp | undefined)?.source ?? "").includes("ai republic") ? provider : standard
    );
    const stub = pageOf({ getByRole });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.cloudProviderButtonFacts()).resolves.toEqual({
      visible: false,
      label: "",
      standardContinueVisible: true,
    });
  });

  it("clicks the provider button", async () => {
    const click = vi.fn(async () => undefined);
    const stub = pageOf({ getByRole: vi.fn(() => locator({ click })) });
    const driver = new WebNewDriver(stub.page);
    await driver.cloudClickProviderButton();
    expect(stub.page.getByRole).toHaveBeenCalledWith("button", { name: /ai republic/i });
    expect(click).toHaveBeenCalledTimes(1);
  });
});
