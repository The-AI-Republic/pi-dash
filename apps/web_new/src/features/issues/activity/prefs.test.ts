// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Feed preferences (ISS-195/196): the stored filter set and sort order
// load once, survive changes in the same tab and fall back to defaults
// when storage is unreadable or refused.
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_FILTERS, FILTERS_STORAGE_KEY, SORT_STORAGE_KEY } from "./model.js";
import { useFeedPrefs } from "./prefs.js";

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  window.localStorage.clear();
});

/**
 * jsdom keeps `getItem`/`setItem` off `Storage.prototype`, so spying on
 * the prototype silently leaves the real storage in place and the
 * refusal branches never run. Replace the whole object instead.
 */
function stubStorage(overrides: Partial<Storage>): void {
  vi.spyOn(window, "localStorage", "get").mockReturnValue({
    ...window.localStorage,
    ...overrides,
  } as unknown as Storage);
}

describe("useFeedPrefs", () => {
  it("starts from every category on and oldest first", () => {
    const { result } = renderHook(() => useFeedPrefs());
    expect(result.current.filters).toEqual([...DEFAULT_FILTERS]);
    expect(result.current.order).toBe("asc");
  });

  it("hydrates a stored selection and order", () => {
    window.localStorage.setItem(FILTERS_STORAGE_KEY, JSON.stringify(["COMMENT"]));
    window.localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify("desc"));
    const { result } = renderHook(() => useFeedPrefs());
    expect(result.current.filters).toEqual(["COMMENT"]);
    expect(result.current.order).toBe("desc");
  });

  it("persists each change as JSON under the shared keys", () => {
    const { result } = renderHook(() => useFeedPrefs());
    act(() => {
      result.current.setFilters(["STATE", "ASSIGNEE"]);
    });
    act(() => {
      result.current.setOrder("desc");
    });
    expect(result.current.filters).toEqual(["STATE", "ASSIGNEE"]);
    expect(result.current.order).toBe("desc");
    expect(window.localStorage.getItem(FILTERS_STORAGE_KEY)).toBe(JSON.stringify(["STATE", "ASSIGNEE"]));
    expect(window.localStorage.getItem(SORT_STORAGE_KEY)).toBe(JSON.stringify("desc"));
  });

  it("keeps the choice in this tab when storage refuses the write", () => {
    stubStorage({
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    });
    const { result } = renderHook(() => useFeedPrefs());
    act(() => {
      result.current.setFilters(["ACTIVITY"]);
    });
    act(() => {
      result.current.setOrder("desc");
    });
    expect(result.current.filters).toEqual(["ACTIVITY"]);
    expect(result.current.order).toBe("desc");
  });

  it("falls back to defaults when storage refuses the read", () => {
    stubStorage({
      getItem: () => {
        throw new Error("SecurityError");
      },
    });
    const { result } = renderHook(() => useFeedPrefs());
    expect(result.current.filters).toEqual([...DEFAULT_FILTERS]);
    expect(result.current.order).toBe("asc");
  });
});
