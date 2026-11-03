// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page display preferences (parity/specs/pages.md §9.1, PAGE-072/035): the
// stored `pages_config` object, its defaults, the canvas measure it drives,
// the local-only merge write (never a server write or a saved-content
// write), and the rule that the formatting bar renders only for an
// editable page with the preference on.

import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import {
  formattingBarVisible,
  PAGE_CANVAS_WIDTH_PX,
  PAGE_PREFERENCES_DEFAULTS,
  PAGE_PREFERENCES_KEY,
  pageCanvasWidth,
  pagePreferencesOf,
  parsePagePreferences,
  readPagePreferencesFromWindow,
  readStickyToolbar,
  readStickyToolbarFromWindow,
  stickyToolbarOf,
  usePagePreferences,
  writePagePreferences,
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

describe("pagePreferencesOf", () => {
  it("fills every absent field from the defaults", () => {
    expect(pagePreferencesOf(null)).toEqual(PAGE_PREFERENCES_DEFAULTS);
    expect(pagePreferencesOf(stored({ full_width: true }))).toEqual({
      ...PAGE_PREFERENCES_DEFAULTS,
      full_width: true,
    });
  });

  it("reads all four stored fields in the old app's value names", () => {
    expect(
      pagePreferencesOf(
        stored({ full_width: true, font_size: "small-font", font_style: "serif", sticky_toolbar: false })
      )
    ).toEqual({ full_width: true, font_size: "small-font", font_style: "serif", sticky_toolbar: false });
  });

  it("falls back for an unknown size and an unknown face", () => {
    expect(pagePreferencesOf(stored({ font_size: "huge" })).font_size).toBe("large-font");
    expect(pagePreferencesOf(stored({ font_style: "comic" })).font_style).toBe("sans-serif");
  });

  it("rejects a non-boolean width and bar rather than truthy-coercing", () => {
    expect(pagePreferencesOf(stored({ full_width: "yes", sticky_toolbar: "no" }))).toMatchObject({
      full_width: false,
      sticky_toolbar: true,
    });
  });
});

describe("readPagePreferencesFromWindow", () => {
  it("reads the complete stored set", () => {
    window.localStorage.setItem(
      PAGE_PREFERENCES_KEY,
      stored({ full_width: true, font_size: "small-font", font_style: "serif", sticky_toolbar: false })
    );
    expect(readPagePreferencesFromWindow()).toEqual({
      full_width: true,
      font_size: "small-font",
      font_style: "serif",
      sticky_toolbar: false,
    });
  });

  it("keeps defaults when storage throws", () => {
    const original = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get: () => {
        throw new Error("denied");
      },
    });
    try {
      expect(readPagePreferencesFromWindow()).toEqual(PAGE_PREFERENCES_DEFAULTS);
    } finally {
      if (original !== undefined) Object.defineProperty(window, "localStorage", original);
    }
  });
});

describe("writePagePreferences", () => {
  it("merges one change and leaves the other three untouched", () => {
    window.localStorage.setItem(
      PAGE_PREFERENCES_KEY,
      stored({ full_width: false, font_size: "small-font", font_style: "serif", sticky_toolbar: true })
    );
    expect(writePagePreferences({ full_width: true })).toEqual({
      full_width: true,
      font_size: "small-font",
      font_style: "serif",
      sticky_toolbar: true,
    });
    expect(JSON.parse(window.localStorage.getItem(PAGE_PREFERENCES_KEY) ?? "{}")).toEqual({
      full_width: true,
      font_size: "small-font",
      font_style: "serif",
      sticky_toolbar: true,
    });
  });

  it("writes only the local key: the stored object carries no record field", () => {
    writePagePreferences({ sticky_toolbar: false });
    const written = window.localStorage.getItem(PAGE_PREFERENCES_KEY) ?? "";
    expect(written).not.toMatch(/description|name|access|logo_props/);
    expect(Object.keys(JSON.parse(written))).toEqual(["full_width", "font_size", "font_style", "sticky_toolbar"]);
  });
});

describe("pageCanvasWidth", () => {
  it("chooses the readable measure unless full width or the pane asks for wide", () => {
    expect(PAGE_CANVAS_WIDTH_PX).toEqual({ readable: "720px", full: "1152px" });
    expect(pageCanvasWidth({ fullWidth: false, paneOpen: false })).toBe("720px");
    expect(pageCanvasWidth({ fullWidth: true, paneOpen: false })).toBe("1152px");
    expect(pageCanvasWidth({ fullWidth: false, paneOpen: true })).toBe("1152px");
    expect(pageCanvasWidth({ fullWidth: true, paneOpen: true })).toBe("1152px");
  });
});

describe("usePagePreferences", () => {
  function harness(pageId = "page-1") {
    return renderHook(({ id }: { id: string }) => usePagePreferences(id), { initialProps: { id: pageId } });
  }

  it("starts from the stored set and reflows on a change", () => {
    window.localStorage.setItem(PAGE_PREFERENCES_KEY, stored({ full_width: false, sticky_toolbar: true }));
    const { result } = harness();
    expect(result.current.preferences.full_width).toBe(false);
    act(() => result.current.setPreference({ full_width: true }));
    expect(result.current.preferences.full_width).toBe(true);
    expect(result.current.preferences.sticky_toolbar).toBe(true);
  });

  it("re-reads the store when the page changes", () => {
    window.localStorage.setItem(PAGE_PREFERENCES_KEY, stored({ font_style: "serif" }));
    const { result, rerender } = harness();
    expect(result.current.preferences.font_style).toBe("serif");
    window.localStorage.setItem(PAGE_PREFERENCES_KEY, stored({ font_style: "sans-serif" }));
    rerender({ id: "page-2" });
    expect(result.current.preferences.font_style).toBe("sans-serif");
  });

  it("survives reload: a fresh mount reads what the previous one wrote", () => {
    const first = harness();
    act(() => first.result.current.setPreference({ font_size: "small-font" }));
    first.unmount();
    const second = harness();
    expect(second.result.current.preferences.font_size).toBe("small-font");
  });
});
