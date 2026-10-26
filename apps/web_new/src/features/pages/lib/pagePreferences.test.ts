// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page display preferences (parity/specs/pages.md §9.1, PAGE-035): the
// stored `pages_config` object, the sticky default, and the rule that the
// formatting bar renders only for an editable page with the preference on.

import { afterEach, describe, expect, it } from "vitest";

import {
  formattingBarVisible,
  PAGE_PREFERENCES_DEFAULTS,
  PAGE_PREFERENCES_KEY,
  parsePagePreferences,
  readStickyToolbar,
  readStickyToolbarFromWindow,
  stickyToolbarOf,
} from "./pagePreferences.js";

afterEach(() => {
  window.localStorage.clear();
});

function stored(value: unknown): string {
  return JSON.stringify(value);
}

describe("parsePagePreferences", () => {
  it("reads an object with the stored fields", () => {
    expect(parsePagePreferences(stored({ sticky_toolbar: false }))).toEqual({ sticky_toolbar: false });
  });

  it.each([
    ["missing", null],
    ["empty", "   "],
    ["unparsable", "{not json"],
    ["null", "null"],
    ["array", "[1,2]"],
    ["string", '"text"'],
    ["number", "3"],
  ])("reads %s as empty", (_name, value) => {
    expect(parsePagePreferences(value)).toEqual({});
  });
});

describe("stickyToolbarOf", () => {
  it("reads each stored boolean", () => {
    expect(stickyToolbarOf(stored({ sticky_toolbar: true }))).toBe(true);
    expect(stickyToolbarOf(stored({ sticky_toolbar: false }))).toBe(false);
  });

  it("defaults to on when unset, absent or not a boolean", () => {
    expect(PAGE_PREFERENCES_DEFAULTS.sticky_toolbar).toBe(true);
    expect(stickyToolbarOf(null)).toBe(true);
    expect(stickyToolbarOf(stored({}))).toBe(true);
    expect(stickyToolbarOf(stored({ sticky_toolbar: "false" }))).toBe(true);
  });
});

describe("formattingBarVisible", () => {
  it("renders only for an editable page with the preference on", () => {
    expect(formattingBarVisible({ editable: true, stickyToolbar: true })).toBe(true);
    expect(formattingBarVisible({ editable: true, stickyToolbar: false })).toBe(false);
    expect(formattingBarVisible({ editable: false, stickyToolbar: true })).toBe(false);
    expect(formattingBarVisible({ editable: false, stickyToolbar: false })).toBe(false);
  });
});

describe("readStickyToolbar", () => {
  it("reads the preference through the platform store", async () => {
    const store = {
      get: (key: string) => Promise.resolve(key === PAGE_PREFERENCES_KEY ? stored({ sticky_toolbar: false }) : null),
      set: () => Promise.resolve(),
      remove: () => Promise.resolve(),
      clear: () => Promise.resolve(),
    };
    await expect(readStickyToolbar(store)).resolves.toBe(false);
  });
});

describe("readStickyToolbarFromWindow", () => {
  it("reads an un-namespaced stored value, defaulting to on", () => {
    expect(readStickyToolbarFromWindow()).toBe(true);
    window.localStorage.setItem(PAGE_PREFERENCES_KEY, stored({ sticky_toolbar: false }));
    expect(readStickyToolbarFromWindow()).toBe(false);
  });

  it("keeps the default where the platform has no window at all", () => {
    const original = globalThis.window;
    // A worker or server render has no window: the preference still reads.
    Reflect.deleteProperty(globalThis, "window");
    try {
      expect(globalThis.window).toBeUndefined();
      expect(readStickyToolbarFromWindow()).toBe(true);
    } finally {
      Object.defineProperty(globalThis, "window", { configurable: true, writable: true, value: original });
    }
  });

  it("keeps the default when storage throws", () => {
    const spy = () => {
      throw new Error("denied");
    };
    const original = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", { configurable: true, get: spy });
    try {
      expect(readStickyToolbarFromWindow()).toBe(true);
    } finally {
      if (original !== undefined) Object.defineProperty(window, "localStorage", original);
    }
  });
});
