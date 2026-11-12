// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for WebNewDriver.favoriteEntryHref (NEWFRONT-144): the
// SHELL-057 unknown-type step reads the fallback href through the
// driver, so the null-when-absent branch is pinned here rather than
// only on the live stack. A stub Page stands in for Playwright; the
// driver only uses getByTestId, locator, first, count, getAttribute.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

/** A driver whose favorites group holds one link (`href`), or none. */
function driverWithLink(href: string | null): WebNewDriver {
  const link = {
    count: async () => (href === null ? 0 : 1),
    getAttribute: async (name: string) => (name === "href" ? href : null),
  };
  const anchored = { first: () => link };
  const row: { locator: () => unknown; first: () => unknown } = {
    locator: () => anchored,
    first: () => row,
  };
  const group = { locator: () => row };
  const page = { getByTestId: () => group };
  return new WebNewDriver(page as unknown as Page);
}

describe("favoriteEntryHref", () => {
  it("returns the entry's href from the sidebar", async () => {
    const driver = driverWithLink("/acme");
    await expect(driver.favoriteEntryHref("Dashboard X")).resolves.toBe("/acme");
  });

  it("returns null when no entry carries the name", async () => {
    const driver = driverWithLink(null);
    await expect(driver.favoriteEntryHref("Missing")).resolves.toBeNull();
  });
});
