// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for the WebDriver last-toast readers (NEWFRONT-404):
// the old app renders its toast roots newest-prepended, so the readers
// must scan first-to-last to honor their "most recently shown"
// contracts. A stub Page stands in for Playwright; the readers only use
// locator, count, nth, and innerText.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";
import { WebDriver } from "./web";

/** A driver whose toast roots carry `texts` in DOM order (index 0 first). */
function driverWithToasts(texts: string[]): WebDriver {
  const roots = {
    count: async () => texts.length,
    nth: (index: number) => ({
      innerText: async () => texts[index] ?? "",
    }),
  };
  const page = { locator: () => roots };
  return new WebDriver(page as unknown as Page);
}

type LastToastReader = "rulesLastToast" | "runnerChatLastToast" | "assistantLastToast";

const readers: LastToastReader[] = ["rulesLastToast", "runnerChatLastToast", "assistantLastToast"];

describe("last-toast readers", () => {
  it.each(readers)("returns the newest toast when two overlap (newest first in DOM): %s", async (reader) => {
    const driver = driverWithToasts(["Error!\nState name is already taken", "Success!\nState created successfully"]);
    await expect(driver[reader]()).resolves.toEqual({
      title: "Error!",
      message: "State name is already taken",
    });
  });

  it("returns the single visible toast", async () => {
    const driver = driverWithToasts(["Success!\nState created successfully"]);
    await expect(driver.rulesLastToast()).resolves.toEqual({
      title: "Success!",
      message: "State created successfully",
    });
  });

  it("skips empty roots", async () => {
    const driver = driverWithToasts(["   ", "Success!\nState created successfully"]);
    await expect(driver.rulesLastToast()).resolves.toEqual({
      title: "Success!",
      message: "State created successfully",
    });
  });

  it("returns null when no toast is visible", async () => {
    const driver = driverWithToasts([]);
    await expect(driver.rulesLastToast()).resolves.toBeNull();
  });

  it("returns null when every root is empty", async () => {
    const driver = driverWithToasts(["", "  \n "]);
    await expect(driver.rulesLastToast()).resolves.toBeNull();
  });
});
