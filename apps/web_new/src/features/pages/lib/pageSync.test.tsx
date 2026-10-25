// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sync indicator seam (parity/specs/pages.md §8.2, PAGE-024): a page starts
// transitional, settles to the fault badge without a live server, and
// adopts whatever the collaboration layer reports for its own page.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PAGE_SYNC_SETTLE_MS, reportPageSync, usePageSyncStatus } from "./pageSync.js";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("usePageSyncStatus", () => {
  it("starts syncing and settles to the fault badge", () => {
    const { result } = renderHook(() => usePageSyncStatus("page-1"));
    expect(result.current).toBe("syncing");
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_SETTLE_MS);
    });
    expect(result.current).toBe("lost");
  });

  it("adopts a report for its own page and ignores other pages", () => {
    const { result } = renderHook(() => usePageSyncStatus("page-1"));
    act(() => {
      reportPageSync("page-2", "synced");
    });
    expect(result.current).toBe("syncing");
    act(() => {
      reportPageSync("page-1", "synced");
    });
    expect(result.current).toBe("synced");
  });

  it("resets to syncing when the address moves to another page", () => {
    const { result, rerender } = renderHook(({ pageId }) => usePageSyncStatus(pageId), {
      initialProps: { pageId: "page-1" },
    });
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_SETTLE_MS);
    });
    expect(result.current).toBe("lost");
    rerender({ pageId: "page-2" });
    expect(result.current).toBe("syncing");
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_SETTLE_MS);
    });
    expect(result.current).toBe("lost");
  });

  it("stops listening once unmounted", () => {
    const { result, unmount } = renderHook(() => usePageSyncStatus("page-1"));
    unmount();
    act(() => {
      reportPageSync("page-1", "synced");
      vi.advanceTimersByTime(PAGE_SYNC_SETTLE_MS);
    });
    expect(result.current).toBe("syncing");
  });
});
