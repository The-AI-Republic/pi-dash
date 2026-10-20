// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the entry card's readiness and failure states
// (NEWFRONT-564, the notifications gate's sign-in flake).
//
// The card paints a loading skeleton inside the same
// `section[aria-label="Sign in"]` it uses once config resolves, and a
// failed config fetch paints a card whose only control is a retry. A
// driver that treats either as a driveable form clicks a submit that is
// not there, and Playwright's unbounded action wait then sits until the
// test times out. These pin the two predicates that separate the states:
// the skeleton is not a painted card, and the failure notice is read.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  first?: () => StubLocator;
  filter?: (options: unknown) => StubLocator;
  count?: () => Promise<number>;
  [key: string]: unknown;
}

function chain(count: number): StubLocator {
  const node: StubLocator = {
    count: async () => count,
    first: () => node,
    filter: () => node,
  };
  return node;
}

/** The selector each call read, so the test can assert what was excluded. */
/** The smallest locator shape the driver's visibility and click reads use. */
function element(overrides: Record<string, unknown>): StubLocator {
  const node = chain(1);
  Object.assign(node, overrides);
  return node;
}

function recordedPage(sectionCount: number, noticeCount: number) {
  const filters: unknown[] = [];
  const section = chain(sectionCount);
  section["filter"] = (options: unknown) => {
    filters.push(options);
    return section;
  };
  const page = {
    locator: vi.fn((_selector: string) => section),
    getByText: vi.fn((_text: unknown) => chain(noticeCount)),
  };
  return { page: page as unknown as Page, filters };
}

describe("paintedCard", () => {
  it("excludes the loading skeleton sharing the card's aria-label", () => {
    const { page, filters } = recordedPage(1, 0);
    const driver = new WebNewDriver(page);

    void (driver as unknown as { paintedCard: () => Locator }).paintedCard();

    expect(page.locator).toHaveBeenCalledWith('section[aria-label="Sign in"], section[aria-label="Sign up"]');
    expect(filters).toHaveLength(1);
    const hasNotText = (filters[0] as { hasNotText: RegExp }).hasNotText;
    expect(hasNotText.test("Loading sign-in options")).toBe(true);
    expect(hasNotText.test("Loading sign-up options")).toBe(true);
    expect(hasNotText.test("Sign in to Pi Dash")).toBe(false);
  });

  it("reads the filtered section through first()", async () => {
    const { page } = recordedPage(1, 0);
    const driver = new WebNewDriver(page);

    const card = (driver as unknown as { paintedCard: () => StubLocator }).paintedCard();

    expect(await card.count!()).toBe(1);
  });
});

describe("instanceConfigFailed", () => {
  it("reads the sign-in failure notice", async () => {
    const { page } = recordedPage(1, 1);
    const driver = new WebNewDriver(page);

    const failed = await (driver as unknown as { instanceConfigFailed: () => Promise<boolean> }).instanceConfigFailed();

    expect(failed).toBe(true);
    const pattern = page.getByText.mock.calls[0]?.[0] as RegExp;
    expect(pattern.test("Could not load sign-in.")).toBe(true);
    expect(pattern.test("Could not load sign-up.")).toBe(true);
    expect(pattern.test("Sign in to Pi Dash")).toBe(false);
  });

  it("is false on the painted card", async () => {
    const { page } = recordedPage(1, 0);
    const driver = new WebNewDriver(page);

    const failed = await (driver as unknown as { instanceConfigFailed: () => Promise<boolean> }).instanceConfigFailed();

    expect(failed).toBe(false);
  });
});

describe("appMounted", () => {
  /** A page whose only interesting call is the mount point's child count. */
  function mountPage(children: number | Error) {
    const locator = {
      evaluate: async (): Promise<boolean> => {
        if (children instanceof Error) throw children;
        return children > 0;
      },
    };
    return { page: { locator: vi.fn((_selector: string) => locator), on: vi.fn() } as unknown as Page };
  }

  it("reads an empty mount point as not mounted", async () => {
    const driver = new WebNewDriver(mountPage(0).page);

    expect(await (driver as unknown as { appMounted: () => Promise<boolean> }).appMounted()).toBe(false);
  });

  it("reads a filled mount point as mounted, whatever it renders", async () => {
    const driver = new WebNewDriver(mountPage(1).page);

    expect(await (driver as unknown as { appMounted: () => Promise<boolean> }).appMounted()).toBe(true);
  });

  it("does not force a reload when the mount point cannot be read", async () => {
    // A navigation mid-evaluate or a detached node must never turn a slow
    // paint into a reload loop.
    const driver = new WebNewDriver(mountPage(new Error("Execution context was destroyed")).page);

    expect(await (driver as unknown as { appMounted: () => Promise<boolean> }).appMounted()).toBe(true);
  });
});

describe("signOutViaCommandPalette", () => {
  it("returns when the page is already on the entry, without pressing the palette", async () => {
    const click = vi.fn(async () => undefined);
    const page = {
      getByPlaceholder: () => element({ isVisible: async () => true }),
      getByRole: () => element({ click }),
    } as unknown as Page;
    const driver = new WebNewDriver(page);

    await driver.signOutViaCommandPalette();

    expect(click).not.toHaveBeenCalled();
  });

  it("gives up with a bounded error instead of waiting on a palette that never appears", async () => {
    const click = vi.fn(async () => {
      throw new Error("timeout");
    });
    const page = {
      getByPlaceholder: () => element({ isVisible: async () => false }),
      getByRole: () => element({ click }),
      keyboard: { press: async () => undefined },
    } as unknown as Page;
    const driver = new WebNewDriver(page);
    vi.spyOn(driver as unknown as { dismissTourIfShown: () => Promise<void> }, "dismissTourIfShown").mockImplementation(
      async () => undefined
    );

    await expect(driver.signOutViaCommandPalette()).rejects.toThrow("sign-out never reached the entry");

    expect(click).toHaveBeenCalledTimes(3);
  });
});

describe("reloadPage", () => {
  it("reloads a document whose mount point stayed empty", async () => {
    const reload = vi.fn(async () => undefined);
    let mounted = false;
    const page = {
      reload,
      locator: () => ({
        evaluate: async () => {
          if (mounted) return true;
          mounted = true;
          return false;
        },
      }),
    } as unknown as Page;
    const driver = new WebNewDriver(page);

    await driver.reloadPage();

    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("leaves a mounted document alone", async () => {
    const reload = vi.fn(async () => undefined);
    const page = {
      reload,
      locator: () => ({ evaluate: async () => true }),
    } as unknown as Page;
    const driver = new WebNewDriver(page);

    await driver.reloadPage();

    expect(reload).toHaveBeenCalledTimes(1);
  });
});
