// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Browser connectivity (parity/specs/pages.md §8.3, PAGE-062): the pill
// follows the browser's online state on editable pages only, delayed by
// the settle beat in both directions, and a flapping connection never
// leaves a stale timer behind.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PAGE_OFFLINE_BEAT_MS, useBrowserOnline, usePageOfflineVisible } from "./pageOffline.js";

const realOnLine = Object.getOwnPropertyDescriptor(window.navigator, "onLine");

function setBrowserOnline(online: boolean): void {
  Object.defineProperty(window.navigator, "onLine", { value: online, configurable: true });
  window.dispatchEvent(new Event(online ? "online" : "offline"));
}

beforeEach(() => {
  vi.useFakeTimers();
  setBrowserOnline(true);
});

afterEach(() => {
  vi.useRealTimers();
  if (realOnLine !== undefined) Object.defineProperty(window.navigator, "onLine", realOnLine);
});

describe("useBrowserOnline", () => {
  it("tracks the browser's online and offline events", () => {
    const { result } = renderHook(() => useBrowserOnline());
    expect(result.current).toBe(true);
    act(() => {
      setBrowserOnline(false);
    });
    expect(result.current).toBe(false);
    act(() => {
      setBrowserOnline(true);
    });
    expect(result.current).toBe(true);
  });

  it("stops listening once unmounted", () => {
    const { result, unmount } = renderHook(() => useBrowserOnline());
    unmount();
    act(() => {
      setBrowserOnline(false);
    });
    expect(result.current).toBe(true);
  });
});

describe("usePageOfflineVisible", () => {
  it("shows the pill a beat after the browser drops on an editable page", () => {
    const { result } = renderHook(() => usePageOfflineVisible(true));
    expect(result.current).toBe(false);
    act(() => {
      setBrowserOnline(false);
    });
    expect(result.current).toBe(false);
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(true);
  });

  it("clears the pill a beat after the browser comes back", () => {
    const { result } = renderHook(() => usePageOfflineVisible(true));
    act(() => {
      setBrowserOnline(false);
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(true);
    act(() => {
      setBrowserOnline(true);
    });
    expect(result.current).toBe(true);
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(false);
  });

  it("never shows the pill on a read-only page", () => {
    const { result } = renderHook(() => usePageOfflineVisible(false));
    act(() => {
      setBrowserOnline(false);
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(false);
  });

  it("drops a pending show when the connection flaps back mid-beat", () => {
    const { result } = renderHook(() => usePageOfflineVisible(true));
    act(() => {
      setBrowserOnline(false);
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS - 1);
    });
    expect(result.current).toBe(false);
    act(() => {
      setBrowserOnline(true);
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS * 2);
    });
    expect(result.current).toBe(false);
  });

  it("hides the pill when the page stops being editable", () => {
    const { result, rerender } = renderHook(({ editable }) => usePageOfflineVisible(editable), {
      initialProps: { editable: true },
    });
    act(() => {
      setBrowserOnline(false);
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(true);
    rerender({ editable: false });
    act(() => {
      vi.advanceTimersByTime(PAGE_OFFLINE_BEAT_MS);
    });
    expect(result.current).toBe(false);
  });
});
