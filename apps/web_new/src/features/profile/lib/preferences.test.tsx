// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab layout and display choices (parity/specs/profile.md §2,
// PROF-006): browser-local, workspace + member scoped, and tolerant of
// partial or corrupted records.
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// jsdom's opaque default origin has no localStorage; give the suite a
// plain in-memory Web Storage so the preference helpers see a browser.
function memoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => {
      entries.clear();
    },
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => {
      entries.delete(key);
    },
    setItem: (key: string, value: string) => {
      entries.set(key, value);
    },
  };
}

Object.defineProperty(globalThis, "localStorage", { value: memoryStorage(), configurable: true });

import {
  DEFAULT_PROFILE_DISPLAY,
  PROFILE_TAB_LAYOUTS,
  profileDisplayKey,
  readProfileDisplay,
  toDisplayPrefs,
  useProfileDisplay,
  writeProfileDisplay,
} from "./preferences.js";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  cleanup();
});

describe("profile display storage", () => {
  it("pins the browser-local key and the offered layouts", () => {
    expect(profileDisplayKey("acme", "u-1")).toBe("pidash:profile-display:acme:u-1:v1");
    expect(PROFILE_TAB_LAYOUTS).toEqual(["list", "kanban"]);
    expect(DEFAULT_PROFILE_DISPLAY).toEqual({
      layout: "list",
      groupBy: null,
      orderBy: "sort_order",
      showEmptyGroups: false,
      properties: {},
    });
  });

  it("reads the defaults when nothing is stored", () => {
    expect(readProfileDisplay("acme", "u-1")).toEqual(DEFAULT_PROFILE_DISPLAY);
  });

  it("round-trips a written record and keeps the two members apart", () => {
    const next = writeProfileDisplay("acme", "u-1", { layout: "kanban", groupBy: "priority" });
    expect(next.layout).toBe("kanban");
    expect(readProfileDisplay("acme", "u-1")).toEqual(next);
    expect(readProfileDisplay("acme", "u-2")).toEqual(DEFAULT_PROFILE_DISPLAY);
    expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
      layout: "kanban",
      groupBy: "priority",
    });
  });

  it("fills gaps in a partial record and drops values of the wrong shape", () => {
    localStorage.setItem(
      profileDisplayKey("acme", "u-1"),
      JSON.stringify({ layout: "board", groupBy: 4, orderBy: "priority", properties: { name: "yes" } })
    );
    expect(readProfileDisplay("acme", "u-1")).toEqual({
      layout: "list",
      groupBy: null,
      orderBy: "priority",
      showEmptyGroups: false,
      properties: {},
    });
  });

  it("accepts a boolean property map and a stored empty-group choice", () => {
    localStorage.setItem(
      profileDisplayKey("acme", "u-1"),
      JSON.stringify({ showEmptyGroups: true, properties: { name: true, priority: false } })
    );
    const read = readProfileDisplay("acme", "u-1");
    expect(read.showEmptyGroups).toBe(true);
    expect(read.properties).toEqual({ name: true, priority: false });
  });

  it("falls back on unreadable JSON and non-object payloads", () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), "{not json");
    expect(readProfileDisplay("acme", "u-1")).toEqual(DEFAULT_PROFILE_DISPLAY);
    localStorage.setItem(profileDisplayKey("acme", "u-1"), "42");
    expect(readProfileDisplay("acme", "u-1")).toEqual(DEFAULT_PROFILE_DISPLAY);
  });

  it("maps the record onto the shared display preferences", () => {
    expect(toDisplayPrefs({ ...DEFAULT_PROFILE_DISPLAY, layout: "kanban", groupBy: "state" })).toEqual({
      layout: "kanban",
      groupBy: "state",
      subGroupBy: null,
      orderBy: "sort_order",
      showEmptyGroups: false,
      showSubIssues: false,
      calendar: { mode: "month", showWeekends: true },
      properties: {},
    });
  });
});

describe("useProfileDisplay", () => {
  it("starts from storage, writes through, and reloads when the member changes", () => {
    writeProfileDisplay("acme", "u-1", { layout: "kanban" });
    const { result, rerender } = renderHook(({ userId }: { userId: string }) => useProfileDisplay("acme", userId), {
      initialProps: { userId: "u-1" },
    });
    expect(result.current[0].layout).toBe("kanban");
    act(() => {
      result.current[1]({ groupBy: "priority" });
    });
    expect(result.current[0]).toMatchObject({ layout: "kanban", groupBy: "priority" });
    expect(readProfileDisplay("acme", "u-1")).toMatchObject({ layout: "kanban", groupBy: "priority" });

    rerender({ userId: "u-2" });
    expect(result.current[0]).toEqual(DEFAULT_PROFILE_DISPLAY);
  });
});
