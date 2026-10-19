// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver marketing reads (NEWFRONT-645,
// CLOUD-003/004). A stub Page stands in for Playwright; every assertion is
// about the selector or time the method reads back, not about a browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: () => Promise<void>;
  waitFor?: () => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  evaluate?: (fn: (root: never) => unknown) => Promise<unknown>;
  focus?: () => Promise<void>;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    evaluate: async () => null,
    focus: async () => undefined,
  };
  node.first = () => node;
  node.nth = () => node;
  node.getByRole = () => node;
  node.locator = () => node;
  Object.assign(node, overrides);
  return node as unknown as Locator;
}

function pageOf(locatorFor: (selector: string) => Locator): Page {
  return {
    locator: (selector: string) => locatorFor(selector),
    getByRole: () => locator(),
    waitForLoadState: async () => undefined,
  } as unknown as Page;
}

describe("cloud frame reads", () => {
  it("reads header links through the first header, with labels and external targets", async () => {
    const anchors = [
      locator({ evaluate: async () => ({ label: "Downloads", href: "/downloads", external: false }) }),
      locator({ evaluate: async () => ({ label: "GitHub", href: "https://github.com/x", external: true }) }),
    ];
    const header = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => anchors.length, nth: (i) => anchors[i]! });
        return locator();
      },
    });
    const page = pageOf((selector) => (selector === "header" ? header : locator()));
    await expect(new WebNewDriver(page).cloudHeaderLinks()).resolves.toEqual([
      { label: "Downloads", href: "/downloads", external: false, visible: true },
      { label: "GitHub", href: "https://github.com/x", external: true, visible: true },
    ]);
  });

  it("skips hidden links and reports false when a visible link has no ring", async () => {
    const hidden = locator({ isVisible: async () => false, evaluate: async () => false });
    const visible = locator({ isVisible: async () => true, evaluate: async () => true });
    const footer = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => 2, nth: (i) => (i === 0 ? hidden : visible) });
        return locator();
      },
    });
    const page = pageOf((selector) => (selector === "footer" ? footer : locator()));
    await expect(new WebNewDriver(page).cloudFooterLinksFocusVisible()).resolves.toBe(true);

    const ringless = locator({ evaluate: async () => false });
    const ringed = locator({ evaluate: async () => true });
    const mixed = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => 2, nth: (i) => (i === 0 ? ringed : ringless) });
        return locator();
      },
    });
    const mixedPage = pageOf((selector) => (selector === "footer" ? mixed : locator()));
    await expect(new WebNewDriver(mixedPage).cloudFooterLinksFocusVisible()).resolves.toBe(false);
  });
});

describe("cloud home reads", () => {
  it("reads the hero action hrefs in DOM order", async () => {
    const links = [
      locator({ getAttribute: async () => "/login" }),
      locator({ getAttribute: async () => "/downloads" }),
      locator({ getAttribute: async () => "/docs" }),
    ];
    const hero = locator({
      getByRole: () => locator({ count: async () => links.length, nth: (i) => links[i]! }),
    });
    const main = locator({
      locator: () =>
        locator({
          first: () => hero,
          locator: () => locator(),
        }),
    });
    const page = pageOf(() => main);
    await expect(new WebNewDriver(page).cloudHomeHeroActionHrefs()).resolves.toEqual(["/login", "/downloads", "/docs"]);
  });

  it("clicks the hero action with the requested href and waits for the next page", async () => {
    const clicked: string[] = [];
    const action = locator({ click: async () => void clicked.push("downloads") });
    const hero = locator({
      locator: (selector: string) => (selector === 'a[href="/downloads"]' ? action : locator()),
    });
    const main = locator({
      locator: (selector: string) => (selector === "section" ? locator({ first: () => hero }) : locator()),
    });
    const page = pageOf(() => main);
    await new WebNewDriver(page).cloudHomeClickHeroAction("/downloads");
    expect(clicked).toEqual(["downloads"]);
  });

  it("counts the strip items, steps, capability cards and reads the repo and closing link", async () => {
    const strip = locator({
      locator: (selector: string) =>
        selector === ":scope > div > div" ? locator({ count: async () => 4 }) : locator(),
    });
    const sections = locator({
      count: async () => 6,
      nth: (index: number) => {
        if (index === 1) return strip;
        if (index === 5) return locator({ getByRole: () => locator({ getAttribute: async () => "/login" }) });
        return locator();
      },
    });
    const main = locator({
      locator: (selector: string) => {
        if (selector === "section") return sections;
        if (selector === "ol > li") return locator({ count: async () => 4 });
        if (selector === "article") return locator({ count: async () => 4 });
        if (selector.includes("github.com")) {
          return locator({
            getAttribute: async (name: string) =>
              name === "href" ? "https://github.com/The-AI-Republic/pi-dash" : "_blank",
          });
        }
        return locator();
      },
    });
    const page = pageOf(() => main);
    await expect(new WebNewDriver(page).cloudHomeSections()).resolves.toEqual({
      stripItems: 4,
      workflowSteps: 4,
      capabilityCards: 4,
      repoHref: "https://github.com/The-AI-Republic/pi-dash",
      repoExternal: true,
      closingCtaHref: "/login",
    });
  });
});
