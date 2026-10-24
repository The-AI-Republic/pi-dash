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

describe("oracle dictation and recurrence fallbacks", () => {
  function driverWith(page: Record<string, unknown>): WebDriver {
    return new WebDriver(page as unknown as Page);
  }

  it("reports the mic and counts speech-config reads", async () => {
    const handlers: ((request: { url(): string; method(): string }) => void)[] = [];
    const node: Record<string, unknown> = {
      count: async () => 1,
      isVisible: async () => true,
    };
    node["locator"] = () => node;
    node["first"] = () => node;
    const page = {
      locator: () => node,
      on: (_event: string, handler: never) => handlers.push(handler),
    };
    const driver = driverWith(page);
    expect(await driver.assistantMicVisible()).toBe(true);
    expect(await driver.assistantSpeechConfigReads()).toBe(0);
    handlers[0]?.({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/stt-config/" });
    expect(await driver.assistantSpeechConfigReads()).toBe(1);
  });

  it("does not count non-GET reads and reports an absent mic", async () => {
    const handlers: ((request: { url(): string; method(): string }) => void)[] = [];
    const absent: Record<string, unknown> = { count: async () => 0 };
    absent["locator"] = () => absent;
    absent["first"] = () => absent;
    const page = {
      locator: () => absent,
      on: (_event: string, handler: never) => handlers.push(handler),
    };
    const driver = driverWith(page);
    expect(await driver.assistantMicVisible()).toBe(false);
    handlers[0]?.({ method: () => "PUT", url: () => "https://app.test/api/users/me/ai-assistant/stt-config/" });
    expect(await driver.assistantSpeechConfigReads()).toBe(0);
  });

  it("flips visibility on refocus", async () => {
    const dispatchEvent = vi.fn(() => true);
    vi.stubGlobal("window", { dispatchEvent });
    vi.stubGlobal("document", {});
    try {
      await driverWith({ evaluate: async (fn: () => void) => fn() }).assistantRefocus();
      expect(dispatchEvent).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reads the plain recurrence field and fills it without a builder", async () => {
    const fill = vi.fn(async () => undefined);
    const raw = {
      count: async () => 1,
      isVisible: async () => true,
      inputValue: async () => "FREQ=DAILY",
      fill,
      first: () => raw,
    };
    const line = { waitFor: async () => undefined, innerText: async () => "every day" };
    const lineNode: Record<string, unknown> = { ...line };
    lineNode["locator"] = () => lineNode;
    lineNode["first"] = () => lineNode;
    const dialog = {
      getByLabel: (label: string) => (label === "Recurrence (RRULE)" ? raw : { count: async () => 0 }),
      locator: () => lineNode,
    };
    const page = { getByRole: () => ({ last: () => dialog }), locator: () => ({ last: () => dialog }) };
    const driver = driverWith(page);
    const facts = await driver.schedulerRecurrenceFacts();
    expect(facts).toMatchObject({ visible: false, rawVisible: true, rawValue: "FREQ=DAILY", sentence: "every day" });
    await expect(driver.schedulerRecurrenceAdvanced()).resolves.toBe(true);
    await driver.schedulerRecurrenceRaw("FREQ=WEEKLY");
    expect(fill).toHaveBeenCalledWith("FREQ=WEEKLY");
  });

  it("answers the unflagged endpoint statuses", async () => {
    const fetchMock = vi.fn(async (_input: string, init?: { method?: string }) => ({
      status: init?.method === "DELETE" ? 204 : 200,
      json: async () => ({ has_api_key: true }),
    }));
    vi.stubGlobal("fetch", fetchMock);
    try {
      const probe = await driverWith({ evaluate: async (fn: () => unknown) => fn() }).assistantDictationApiProbe();
      expect(probe).toEqual({ read: 200, readEnabled: null, save: 200, test: 200, transcribe: 200, remove: 204 });
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
