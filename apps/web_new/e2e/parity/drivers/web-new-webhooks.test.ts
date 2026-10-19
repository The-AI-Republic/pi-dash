// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver webhook methods (NEWFRONT-541,
// WSET-019-024/041). A stub Page stands in for Playwright; each assertion
// is about the address the method visits, the selector it reads or the
// value it returns.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isChecked?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  inputValue?: (options?: unknown) => Promise<string>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  allInnerTexts?: () => Promise<string[]>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  locator?: (selector: string) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  getByLabel?: (text: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isChecked: async () => true,
    click: async () => undefined,
    fill: async () => undefined,
    inputValue: async () => "",
    getAttribute: async () => null,
    waitFor: async () => undefined,
    allInnerTexts: async () => [],
    innerText: async () => "",
    textContent: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    locator: () => node,
    getByRole: () => node,
    getByText: () => node,
    getByLabel: () => node,
    getByTestId: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

interface Harness {
  page: Page;
  visited: string[];
  pressed: string[];
  routeCalls: string[];
  clicks: string[];
  fills: Array<{ selector: string; value: string }>;
  granted: string[][];
}

function pageOf(
  parts: {
    byRole?: (role: string, name: string | RegExp | undefined) => Locator;
    byText?: (text: string) => Locator;
    byLabel?: (label: string) => Locator;
    byTestId?: (id: string) => Locator;
    bySelector?: (selector: string) => Locator;
    keyboard?: Page["keyboard"];
  } = {}
): Harness {
  const state: Harness = {
    page: null as unknown as Page,
    visited: [],
    pressed: [],
    routeCalls: [],
    clicks: [],
    fills: [],
    granted: [],
  };
  const click = async (name: string) => {
    state.clicks.push(name);
  };
  const locatorNode = (selector: string, options: unknown) =>
    locator({
      click: async () => {
        await click(selector);
      },
      fill: async (value: unknown) => {
        state.fills.push({ selector, value: String(value) });
      },
      getByRole: (role: string, o: unknown) => roleNode(role, (o as { name?: string } | undefined)?.name),
      getByText: (text: string) => parts.byText?.(text) ?? locator(),
      getByLabel: (label: string) => parts.byLabel?.(label) ?? locator(),
      fill2: options,
    });
  const roleNode = (role: string, name: string | RegExp | undefined) =>
    parts.byRole?.(role, name) ??
    locator({
      click: async () => {
        await click(`${role}:${String(name)}`);
      },
      fill: async (value: unknown) => {
        state.fills.push({ selector: `${role}:${String(name)}`, value: String(value) });
      },
    });
  state.page = {
    goto: async (url: string) => {
      state.visited.push(url);
      return null;
    },
    waitForLoadState: async () => undefined,
    getByRole: (role: string, options?: { name?: string | RegExp }) => roleNode(role, options?.name),
    getByText: (text: string) => parts.byText?.(text) ?? locator(),
    getByTestId: (id: string) => parts.byTestId?.(id) ?? locator(),
    getByLabel: (label: string) => parts.byLabel?.(label) ?? locator(),
    locator: (selector: string) => parts.bySelector?.(selector) ?? locatorNode(selector, undefined),
    keyboard:
      parts.keyboard ??
      ({
        press: async (key: string) => {
          state.pressed.push(key);
        },
      } as unknown as Page["keyboard"]),
    route: async (pattern: string) => {
      state.routeCalls.push(pattern);
    },
    waitForResponse: async () => null,
    waitForEvent: async () => null,
    context: () => ({
      grantPermissions: async (permissions: string[]) => {
        state.granted.push(permissions);
      },
    }),
    url: () => "https://p.test/acme/settings/webhooks",
    evaluate: async (fn: (arg?: unknown) => unknown) => fn(),
  } as unknown as Page;
  return state;
}

describe("web_new webhook driver", () => {
  it("opens the list and settles on its Add control", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsOpenWebhooks("acme");
    expect(harness.visited).toEqual(["/acme/settings/webhooks"]);
  });

  it("opens a detail at its own address", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsOpenWebhookDetail("acme", "wh-1");
    expect(harness.visited).toEqual(["/acme/settings/webhooks/wh-1"]);
  });

  it("reads the webhook list surface from the heading, verb and copy", async () => {
    const harness = pageOf({
      byRole: (role, name) => locator({ count: async () => (role === "heading" && name === "Webhooks" ? 1 : 0) }),
      byText: (text) => locator({ count: async () => (text === "No Webhook added yet" ? 1 : 0) }),
      byTestId: (id) => locator({ count: async () => (id === "workspace-settings-not-authorized" ? 0 : 1) }),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhooksHeadingVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhooksEmptyVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhooksNotAuthorized()).resolves.toBe(false);
  });

  it("reads the loader from the status region and holds the list route", async () => {
    const harness = pageOf({
      byRole: (role, name) =>
        locator({ count: async () => (role === "status" && name === "Loading webhooks" ? 1 : 0) }),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhooksLoaderVisible()).resolves.toBe(true);
    await driver.wsSettingsWebhooksDelayListFetch(1);
    expect(harness.routeCalls).toEqual(["**/api/workspaces/*/webhooks/"]);
  });

  it("gathers row URLs only from detail links", async () => {
    const rows = [
      { href: "/acme/settings/webhooks/1", text: "https://example.com/a" },
      { href: "/acme/settings/webhooks", text: "Webhooks" },
      { href: "/acme/settings/webhooks/2", text: "https://example.com/b" },
    ];
    const harness = pageOf({
      bySelector: (selector) =>
        selector === 'a[href*="/settings/webhooks/"]'
          ? locator({
              count: async () => rows.length,
              nth: (index) =>
                locator({
                  getAttribute: async () => rows[index]?.href ?? null,
                  innerText: async () => rows[index]?.text ?? "",
                }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhooksRowUrls()).resolves.toEqual([
      "https://example.com/a",
      "https://example.com/b",
    ]);
  });

  it("flips the row switch by URL", async () => {
    const clicked: string[] = [];
    const harness = pageOf({
      byRole: (role, name) =>
        locator({
          getByRole: (nested: string) =>
            locator({
              click: async () => {
                clicked.push(`${nested}:${String(name)}`);
              },
            }),
        }),
    });
    await new WebNewDriver(harness.page).wsSettingsWebhooksRowToggleActive("https://example.com/a");
    expect(clicked).toEqual(["switch:https://example.com/a"]);
  });

  it("opens the create dialog and reads its title and event mode", async () => {
    const harness = pageOf({
      byText: (text) => locator({ count: async () => (text === "Create webhook" ? 1 : 0) }),
      bySelector: (selector) =>
        selector === "form"
          ? locator({
              locator: (nested: string) => locator({ isChecked: async () => nested === "input#individual" }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhooksAddClick();
    await expect(driver.wsSettingsWebhookDialogTitle()).resolves.toBe("Create webhook");
    await expect(driver.wsSettingsWebhookEventMode()).resolves.toBe("individual");
    await driver.wsSettingsWebhookPickEventMode("all");
    expect(harness.clicks).toContain("button:Add webhook");
    expect(harness.clicks.length).toBe(1);
  });

  it("fills the target address and sends Escape", async () => {
    const harness = pageOf({
      byLabel: (label) => locator({ inputValue: async () => (label === "Target URL" ? "https://example.com/a" : "") }),
    });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookUrlFill("https://example.com/a");
    await expect(driver.wsSettingsWebhookUrlValue()).resolves.toBe("https://example.com/a");
    await driver.wsSettingsWebhookPressEscape();
    expect(harness.pressed).toEqual(["Escape"]);
  });

  it("reads the reveal copy and the in-clear key", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "[data-secret-value]"
          ? locator({ innerText: async () => "whsec_fixture_key_1234567890" })
          : locator(),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhookSecretShown()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookSecretValue()).resolves.toBe("whsec_fixture_key_1234567890");
  });

  it("copies through the granted clipboard", async () => {
    const harness = pageOf();
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookSecretCopy();
    expect(harness.granted).toEqual([["clipboard-read", "clipboard-write"]]);
  });

  it("reads the detail loader while the page is still loading", async () => {
    const harness = pageOf({
      byRole: (role) => locator({ count: async () => (role === "status" ? 1 : 0) }),
      byTestId: () => locator({ count: async () => 0 }),
      byText: () => locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookDetailLoaderVisible()).resolves.toBe(true);
  });

  it("reads the detail form's active switch", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? locator({
              getByRole: () => locator({ count: async () => 1, isChecked: async () => true }),
            })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookDetailActiveOn()).resolves.toBe(true);
  });

  it("reads the update and regenerate labels", async () => {
    const harness = pageOf({
      byRole: (role, name) => locator({ innerText: async () => `${role}:${String(name)}` }),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhookDetailUpdateLabel()).resolves.toMatch(/Update/);
    await expect(driver.wsSettingsWebhookRegenerateLabel()).resolves.toMatch(/Re-generat/);
  });

  it("expands the danger zone and reads its confirmation copy", async () => {
    const harness = pageOf({
      byText: (text) => locator({ count: async () => (text === "Once a webhook is deleted" ? 1 : 0) }),
      byRole: (role, name) =>
        locator({
          count: async () => (role === "button" && name === "Delete webhook" ? 1 : 0),
        }),
    });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookDeleteSectionExpand();
    await expect(driver.wsSettingsWebhookDeletePanelVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookDeleteDialogText()).resolves.toBeNull();
  });

  it("reports the detail state from the denial and form markers", async () => {
    const denied = pageOf({
      byTestId: (id) => locator({ count: async () => (id === "workspace-settings-not-authorized" ? 1 : 0) }),
    });
    await expect(new WebNewDriver(denied.page).wsSettingsWebhookDetailState()).resolves.toBe("denied");
    const form = pageOf({
      byTestId: () => locator({ count: async () => 0 }),
      byText: (text) => locator({ count: async () => (text === "Webhook details" ? 1 : 0) }),
    });
    await expect(new WebNewDriver(form.page).wsSettingsWebhookDetailState()).resolves.toBe("form");
    const loading = pageOf({
      byTestId: () => locator({ count: async () => 0 }),
      byText: () => locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(loading.page).wsSettingsWebhookDetailState()).resolves.toBe("loading");
  });

  it("returns the newest toast text", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector.includes('aria-label="Notifications"')
          ? locator({ count: async () => 1, innerText: async () => "Webhook created" })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(harness.page).lastToast()).resolves.toBe("Webhook created");
  });

  it("returns null when no toast is showing", async () => {
    const harness = pageOf({ bySelector: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(harness.page).lastToast()).resolves.toBeNull();
  });
});
