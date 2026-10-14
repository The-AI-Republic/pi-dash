// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for WebDriver.setFavoritesOpen's verify-and-re-drive
// loop (NEWFRONT-296): after a reload the open-state read can run while
// the favorites fetch is still in flight, so the section is hidden and
// reads closed; the click then waits for the toggle and lands after the
// section mounts already open (its flag persists), closing it instead.
// A stub Page stands in for Playwright and scripted open-state reads
// stand in for the old app; the driver only uses locator, evaluateAll,
// nth, first, getByRole, click, waitForFunction, and waitForTimeout.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";
import { WebDriver } from "./web";

interface ScriptedPage {
  clicks: number;
  settles: number;
  clickImpl: () => Promise<void>;
  page: Page;
}

/** A stub page whose sidebar aside is always on screen. Clicks are counted. */
function scriptedPage(): ScriptedPage {
  const state = {
    clicks: 0,
    settles: 0,
    clickImpl: async () => {},
  };
  const toggle = {
    click: async () => {
      state.clicks += 1;
      await state.clickImpl();
    },
  };
  const aside = { getByRole: () => ({ first: () => toggle }) };
  const asides = {
    evaluateAll: async () => [[0, 100, 100]],
    nth: () => aside,
    first: () => aside,
  };
  const page = {
    locator: () => asides,
    waitForFunction: async () => {},
    waitForTimeout: async () => {
      state.settles += 1;
    },
  };
  return Object.assign(state, { page: page as unknown as Page });
}

/** A driver whose open-state reads replay `states`, holding the last one. */
function driverWithStates(script: ScriptedPage, states: boolean[]): WebDriver {
  const queue = [...states];
  const driver = new WebDriver(script.page);
  const scripted = driver as unknown as { isFavoritesOpen: () => Promise<boolean> };
  scripted.isFavoritesOpen = async () => (queue.length > 1 ? (queue.shift() ?? false) : (queue[0] ?? false));
  return driver;
}

describe("setFavoritesOpen", () => {
  it("returns without clicking when already open", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [true]);
    await driver.setFavoritesOpen(true);
    expect(script.clicks).toBe(0);
    expect(script.settles).toBe(0);
  });

  it("returns without clicking when already closed", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [false]);
    await driver.setFavoritesOpen(false);
    expect(script.clicks).toBe(0);
    expect(script.settles).toBe(0);
  });

  it("drives once and converges when closed", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [false, true]);
    await driver.setFavoritesOpen(true);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(1);
  });

  it("drives once and converges when closing", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [true, false]);
    await driver.setFavoritesOpen(false);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(1);
  });

  it("re-drives when the first click lands after an already-open mount", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [false, false, true]);
    await driver.setFavoritesOpen(true);
    expect(script.clicks).toBe(2);
    expect(script.settles).toBe(2);
  });

  it("swallows a click timeout and still converges", async () => {
    const script = scriptedPage();
    script.clickImpl = async () => {
      throw new Error("locator.click: Timeout 15000ms exceeded");
    };
    const driver = driverWithStates(script, [false, true]);
    await driver.setFavoritesOpen(true);
    expect(script.clicks).toBe(1);
  });

  it("converges when the third drive lands", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [false, false, false, true]);
    await driver.setFavoritesOpen(true);
    expect(script.clicks).toBe(3);
    expect(script.settles).toBe(3);
  });

  it("throws an honest error when the section never converges", async () => {
    const script = scriptedPage();
    const driver = driverWithStates(script, [false]);
    await expect(driver.setFavoritesOpen(true)).rejects.toThrow("never reached open=true");
    expect(script.clicks).toBe(3);
    expect(script.settles).toBe(3);
  });
});
