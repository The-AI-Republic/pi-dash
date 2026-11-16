// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SPACE_AGE_TICK_MS, useRelativeAge } from "./useRelativeAge.js";

afterEach(() => {
  vi.useRealTimers();
});

describe("useRelativeAge", () => {
  it("updates the relative age on the minute cadence and clears its timer", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-26T12:00:00Z"));
    const createdAt = "2026-10-26T11:00:00Z";
    const { result, unmount } = renderHook(() => useRelativeAge(createdAt));
    expect(result.current).toBe("1 hour ago");

    act(() => {
      vi.advanceTimersByTime(SPACE_AGE_TICK_MS);
    });
    expect(result.current).toBe("1 hour ago");

    act(() => {
      vi.advanceTimersByTime(59 * SPACE_AGE_TICK_MS);
    });
    expect(result.current).toBe("2 hours ago");

    unmount();
    act(() => {
      vi.advanceTimersByTime(SPACE_AGE_TICK_MS);
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("accepts an explicit cadence", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-26T12:00:00Z"));
    const { result } = renderHook(() => useRelativeAge("2026-10-26T11:59:00Z", 1_000));
    expect(result.current).toBe("1 minute ago");
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(result.current).toBe("2 minutes ago");
  });
});
