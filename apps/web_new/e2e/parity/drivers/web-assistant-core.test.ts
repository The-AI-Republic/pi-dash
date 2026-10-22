// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebDriver assistant-core readers (NEWFRONT-605,
// AGT-047): the error-tint probe reads the rendered colour rather than a
// class name, since the two apps spell their danger token differently.
// A stub Page stands in for Playwright.
import type { Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebDriver } from "./web";

function driverWithTintNode(node: unknown): WebDriver {
  const row = { evaluate: async (fn: (target: Element) => unknown) => fn(node as Element) };
  const list = { locator: () => ({ nth: () => row }) };
  const column = { locator: () => list };
  const page = { locator: () => column };
  return new WebDriver(page as unknown as Page);
}

async function withComputedColor(color: string, check: (seen: unknown[]) => Promise<void>): Promise<void> {
  const seen: unknown[] = [];
  const getComputedStyle = vi.fn((element: Element) => {
    seen.push(element);
    return { color };
  });
  const scope = globalThis as Record<string, unknown>;
  const previous = scope.getComputedStyle;
  scope.getComputedStyle = getComputedStyle;
  try {
    await check(seen);
  } finally {
    if (previous === undefined) delete scope.getComputedStyle;
    else scope.getComputedStyle = previous;
  }
}

describe("assistant error tint", () => {
  it("reads the inner box colour when the row wraps one", async () => {
    const box = {};
    const node = { firstElementChild: box };
    await withComputedColor("rgb(185, 28, 28)", async (seen) => {
      await expect(driverWithTintNode(node).assistantErrorTint(0)).resolves.toBe("rgb(185, 28, 28)");
      expect(seen).toEqual([box]);
    });
  });

  it("reads the row itself when it has no inner box", async () => {
    const node = { firstElementChild: null };
    await withComputedColor("rgb(185, 28, 28)", async (seen) => {
      await expect(driverWithTintNode(node).assistantErrorTint(0)).resolves.toBe("rgb(185, 28, 28)");
      expect(seen).toEqual([node]);
    });
  });
});
