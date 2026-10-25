// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebDriver sides of the NEWFRONT-907 scenarios: the
// sub-work-items Load more reader (ISS-238) and the new-app-first probes
// that must refuse loudly instead of reading the old UI (ISS-239–242). A
// stub Page stands in for Playwright.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebDriver } from "./web.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  innerText?: () => Promise<string>;
  getByText?: (text: string, options?: unknown) => StubLocator;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    innerText: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    getByText: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

/** A page whose header button and section content are the given stubs. */
function pageOf(section: StubLocator): Page {
  return {
    locator: (selector: string) => (selector === "button.w-full" ? section : locator()),
    waitForTimeout: async () => undefined,
  } as unknown as Page;
}

describe("sub-work-items Load more on the old app (ISS-238)", () => {
  it("reads the section's Load more text", async () => {
    const page = pageOf(locator({ count: async () => 1 }));
    await expect(new WebDriver(page).widgetLoadMorePresent("Sub-work items")).resolves.toBe(true);
  });

  it("reads no control when the section carries none", async () => {
    const page = pageOf(locator({ count: async () => 0 }));
    await expect(new WebDriver(page).widgetLoadMorePresent("Sub-work items")).resolves.toBe(false);
  });

  it("clicks the control and returns once the rows grow", async () => {
    const clicks: string[] = [];
    const row = locator({ count: async () => 1, click: async () => void clicks.push("load") });
    const content = locator({ getByText: () => row });
    const section = locator({ locator: () => content });
    const page = {
      locator: () => locator({ locator: () => section }),
      waitForTimeout: async () => undefined,
    } as unknown as Page;
    const driver = new WebDriver(page);
    // The row list grows only after the second read, the way a landed page
    // renders; the control stays until then.
    let reads = 0;
    driver.widgetRowNames = async () => {
      reads += 1;
      return reads <= 1 ? ["row-1"] : ["row-1", "row-2"];
    };
    await driver.widgetLoadMore("Sub-work items");
    expect(clicks).toEqual(["load"]);
    expect(reads).toBe(2);
  });

  it("refuses to load more without a control", async () => {
    const page = pageOf(locator({ count: async () => 0 }));
    await expect(new WebDriver(page).widgetLoadMore("Sub-work items")).rejects.toThrow("no load-more row");
  });
});

describe("new-app-first probes refuse on the old app", () => {
  const page = pageOf(locator());
  const driver = new WebDriver(page);

  it.each([
    ["waitingBannerPresent", () => driver.waitingBannerPresent()],
    ["waitingBannerText", () => driver.waitingBannerText()],
    ["waitingBannerActions", () => driver.waitingBannerActions()],
    ["waitingBannerActionDisabled", () => driver.waitingBannerActionDisabled("Cancel run")],
    ["waitingBannerAction", () => driver.waitingBannerAction("Cancel run")],
    ["propertyInlineError", () => driver.propertyInlineError("Runs on")],
    ["pickerOptionReason", () => driver.pickerOptionReason("nf907-online")],
  ])("%s names PDASHOSS01-299", async (_name, read) => {
    await expect(read()).rejects.toThrow("PDASHOSS01-299");
  });
});
