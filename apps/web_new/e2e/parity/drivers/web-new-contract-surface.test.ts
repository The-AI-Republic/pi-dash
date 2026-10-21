// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// @vitest-environment jsdom
// Unit tests for the web_new driver's desktop-runtime reads (NEWFRONT-207,
// DESK-003/010/026) and a guard that the class keeps them.
//
// A notifications-gate edit once replaced a block of throwing stubs with its
// own code and took these three reads with it. Nothing failed: tsconfig only
// typechecks `src`, and the desktop scenarios run in a different area's gate.
// These tests make the loss visible in this area's unit suite.
import type { Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

const originalIndexedDb = (globalThis as { indexedDB?: unknown }).indexedDB;

afterEach(() => {
  (globalThis as { indexedDB?: unknown }).indexedDB = originalIndexedDb;
});

/** A page whose body text is `body`. */
function textPage(body: string) {
  const innerText = vi.fn(async () => body);
  const page = { locator: vi.fn((_selector: string) => ({ innerText })) };
  return { page: page as unknown as Page, innerText };
}

/** A page that runs the evaluated callback in this process's globals. */
function evaluatingPage() {
  const evaluate = vi.fn(async <T>(callback: () => T): Promise<T> => callback());
  const page = { evaluate };
  return { page: page as unknown as Page, evaluate };
}

describe("deskRuntimePageText", () => {
  it("reads the rendered body and trims it", async () => {
    const { page, innerText } = textPage("  No repair needed\n");

    const text = await new WebNewDriver(page).deskRuntimePageText();

    expect(text).toBe("No repair needed");
    expect(innerText).toHaveBeenCalledTimes(1);
  });
});

describe("deskRuntimeDispatchWindowFocus", () => {
  it("dispatches one focus event inside the page", async () => {
    const { page, evaluate } = evaluatingPage();
    let received: Event | null = null;
    globalThis.addEventListener("focus", (event) => (received = event), { once: true });

    await new WebNewDriver(page).deskRuntimeDispatchWindowFocus();

    expect(evaluate).toHaveBeenCalledTimes(1);
    expect(received?.type).toBe("focus");
  });
});

describe("deskRuntimeIndexedDatabaseNames", () => {
  it("lists named client databases and drops unnamed entries", async () => {
    const { page } = evaluatingPage();
    (globalThis as { indexedDB?: unknown }).indexedDB = {
      databases: async () => [{ name: "chat-history" }, { name: "" }, { name: null }],
    };

    expect(await new WebNewDriver(page).deskRuntimeIndexedDatabaseNames()).toEqual(["chat-history"]);
  });

  it("reports nothing when the browser cannot list its databases", async () => {
    const { page } = evaluatingPage();
    (globalThis as { indexedDB?: unknown }).indexedDB = {};

    expect(await new WebNewDriver(page).deskRuntimeIndexedDatabaseNames()).toEqual([]);
  });
});

describe("driver surface", () => {
  it("keeps the desktop-runtime reads on the class itself", () => {
    // tsconfig only typechecks `src`, so removing these broke nothing until
    // the desktop gate ran DESK-003/010/026 against web_new. The prototype
    // read fails loudly in this suite if that happens again.
    const methods = Object.getOwnPropertyNames(Object.getPrototypeOf(new WebNewDriver({} as unknown as Page)));
    for (const name of ["deskRuntimePageText", "deskRuntimeDispatchWindowFocus", "deskRuntimeIndexedDatabaseNames"]) {
      expect(methods, name).toContain(name);
    }
  });
});
