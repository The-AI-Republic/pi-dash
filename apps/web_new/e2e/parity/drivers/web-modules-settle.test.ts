// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for WebDriver.modulesAwaitFetchQuiescent (NEWFRONT-370):
// the modules list fires its collection fetch twice on load and merges every
// response over the store, so a star click between the row render and the
// trailing response gets its optimistic update clobbered by stale data. The
// driver settles the fetches before clicking: it counts in-flight modules
// requests and returns only after a quiet window with none in flight and
// none newly started. A stub Page stands in for Playwright; scripted
// request/finish/fail events stand in for the old app, and waitForTimeout
// ticks instantly so the quiet window costs no wall time.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";
import { WebDriver } from "./web";

type EventHandler = (event: { url(): string }) => void;

interface ScriptedSettlePage {
  waits: number;
  offs: string[];
  emit: (event: string, url: string) => void;
  page: Page;
}

/** A stub page that records listeners and ticks instantly through waits. */
function scriptedSettlePage(hook?: (waits: number) => void): ScriptedSettlePage {
  const state = {
    waits: 0,
    offs: [] as string[],
    handlers: new Map<string, EventHandler[]>(),
  };
  const emit = (event: string, url: string): void => {
    for (const handler of state.handlers.get(event) ?? []) handler({ url: () => url });
  };
  const page = {
    on: (event: string, handler: EventHandler) => {
      state.handlers.set(event, [...(state.handlers.get(event) ?? []), handler]);
    },
    off: (event: string, handler: EventHandler) => {
      state.offs.push(event);
      state.handlers.set(
        event,
        (state.handlers.get(event) ?? []).filter((held) => held !== handler)
      );
    },
    waitForTimeout: async () => {
      state.waits += 1;
      hook?.(state.waits);
    },
  };
  return Object.assign(state, { emit, page: page as unknown as Page });
}

/** The private settle exposed for focused tests (same trick as web-favorites). */
function settleOf(driver: WebDriver): () => Promise<void> {
  return (driver as unknown as { modulesAwaitFetchQuiescent: () => Promise<void> }).modulesAwaitFetchQuiescent.bind(
    driver
  );
}

const MODULES_URL = "http://localhost:13070/api/workspaces/ws/projects/p/modules/";
const OTHER_URL = "http://localhost:13070/api/workspaces/ws/user-favorites/?all=true";

describe("modulesAwaitFetchQuiescent", () => {
  it("returns after one quiet window when nothing is in flight", async () => {
    const script = scriptedSettlePage();
    const driver = new WebDriver(script.page);
    await settleOf(driver)();
    // Ten quiet ticks; the tenth returns without waiting again.
    expect(script.waits).toBe(9);
    expect(script.offs.sort()).toEqual(["request", "requestfailed", "requestfinished"]);
  });

  it("waits out an in-flight round before the quiet window", async () => {
    const holder: { script?: ScriptedSettlePage } = {};
    const script = scriptedSettlePage((count) => {
      if (count === 3) holder.script?.emit("request", MODULES_URL);
      if (count === 5) holder.script?.emit("requestfinished", MODULES_URL);
    });
    holder.script = script;
    const driver = new WebDriver(script.page);
    await settleOf(driver)();
    // Three ticks, a two-tick round, then a fresh ten-tick window.
    expect(script.waits).toBe(14);
    expect(script.offs.sort()).toEqual(["request", "requestfailed", "requestfinished"]);
  });

  it("restarts the window when a response lands for an unseen request", async () => {
    const holder: { script?: ScriptedSettlePage } = {};
    // A round already in flight when the listeners attach: its request is
    // missed, so the finish must still restart the window (and never drive
    // the in-flight count negative).
    const script = scriptedSettlePage((count) => {
      if (count === 2) holder.script?.emit("requestfinished", MODULES_URL);
    });
    holder.script = script;
    const driver = new WebDriver(script.page);
    await settleOf(driver)();
    expect(script.waits).toBe(11);
  });

  it("treats a failed request as settled", async () => {
    const holder: { script?: ScriptedSettlePage } = {};
    const script = scriptedSettlePage((count) => {
      if (count === 2) holder.script?.emit("request", MODULES_URL);
      if (count === 4) holder.script?.emit("requestfailed", MODULES_URL);
    });
    holder.script = script;
    const driver = new WebDriver(script.page);
    await settleOf(driver)();
    expect(script.waits).toBe(13);
  });

  it("ignores non-modules traffic", async () => {
    const holder: { script?: ScriptedSettlePage } = {};
    const script = scriptedSettlePage((count) => {
      if (count === 3) {
        holder.script?.emit("request", OTHER_URL);
        holder.script?.emit("requestfinished", OTHER_URL);
        holder.script?.emit("requestfailed", OTHER_URL);
      }
    });
    holder.script = script;
    const driver = new WebDriver(script.page);
    await settleOf(driver)();
    expect(script.waits).toBe(9);
  });

  it("throws an honest error when the fetches never settle", async () => {
    const holder: { script?: ScriptedSettlePage } = {};
    const script = scriptedSettlePage((count) => {
      if (count === 1) holder.script?.emit("request", MODULES_URL);
    });
    holder.script = script;
    const driver = new WebDriver(script.page);
    await expect(settleOf(driver)()).rejects.toThrow("never settled");
    expect(script.waits).toBe(300);
    expect(script.offs.sort()).toEqual(["request", "requestfailed", "requestfinished"]);
  });
});

describe("modulesToggleStar", () => {
  it("settles the list fetches before clicking the star", async () => {
    const order: string[] = [];
    const chain: {
      last(): unknown;
      first(): unknown;
      filter(): unknown;
      locator(): unknown;
      click(): Promise<void>;
    } = {
      last: () => chain,
      first: () => chain,
      filter: () => chain,
      locator: () => chain,
      click: async () => {
        order.push("click");
      },
    };
    const page = { locator: () => chain };
    const driver = new WebDriver(page as unknown as Page);
    const scripted = driver as unknown as {
      modulesAwaitFetchQuiescent: () => Promise<void>;
      modulesLastToast: () => Promise<{ title: string; message: string } | null>;
    };
    scripted.modulesAwaitFetchQuiescent = async () => {
      order.push("settle");
    };
    scripted.modulesLastToast = async () => ({ title: "Success!", message: "Module added to favorites." });
    await driver.modulesToggleStar("Starred Module");
    expect(order).toEqual(["settle", "click"]);
  });
});
