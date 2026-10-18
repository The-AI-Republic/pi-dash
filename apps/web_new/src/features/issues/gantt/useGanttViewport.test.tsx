// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DAY_WIDTHS, EXTENSION_DAYS, centeredScroll, initialRange, timelineWidth } from "./scale.js";
import { FALLBACK_VIEWPORT_WIDTH, clampScroll, useGanttViewport } from "./useGanttViewport.js";

const TODAY = "2026-10-18";

describe("useGanttViewport", () => {
  it("opens at Week with today inside the window", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    expect(result.current.zoom).toBe("week");
    expect(result.current.range).toEqual(initialRange("week", TODAY, 0));
    expect(result.current.scrollLeft).toBe(0);
  });

  it("switches zoom to a new window and re-centers on measurement", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(900));
    const weekOffset = result.current.scrollLeft;
    expect(weekOffset).toBe(centeredScroll(TODAY, initialRange("week", TODAY, 0), "week", 900));
    act(() => result.current.setZoom("month"));
    expect(result.current.zoom).toBe("month");
    expect(result.current.range).toEqual(initialRange("month", TODAY, 0));
    act(() => result.current.measure(900));
    expect(result.current.scrollLeft).toBe(centeredScroll(TODAY, initialRange("month", TODAY, 0), "month", 900));
  });

  it("keeps the view still on a resize after the first measure", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(900));
    const settled = result.current.scrollLeft;
    act(() => result.current.measure(1200));
    expect(result.current.scrollLeft).toBe(settled);
  });

  it("ignores a zero measurement", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(0));
    expect(result.current.scrollLeft).toBe(0);
    act(() => result.current.measure(800));
    expect(result.current.scrollLeft).toBe(centeredScroll(TODAY, initialRange("week", TODAY, 0), "week", 800));
  });

  it("re-centers on Today using the measured width", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(900));
    act(() => result.current.setScrollLeft(0));
    act(() => result.current.centerToday());
    expect(result.current.scrollLeft).toBe(centeredScroll(TODAY, initialRange("week", TODAY, 0), "week", 900));
  });

  it("uses the fallback width before a measurement", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.centerToday());
    expect(result.current.scrollLeft).toBe(
      centeredScroll(TODAY, initialRange("week", TODAY, 0), "week", FALLBACK_VIEWPORT_WIDTH)
    );
  });

  it("appends days on the right without moving the offset", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(900));
    act(() => result.current.setScrollLeft(500));
    const before = result.current.range;
    act(() => result.current.extend("right"));
    expect(result.current.range.dayCount).toBe(before.dayCount + EXTENSION_DAYS.week);
    expect(result.current.scrollLeft).toBe(500);
  });

  it("compensates the offset when prepending days on the left", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 0));
    act(() => result.current.measure(900));
    act(() => result.current.setScrollLeft(500));
    const before = result.current.range;
    act(() => result.current.extend("left"));
    expect(result.current.range.startIso < before.startIso).toBe(true);
    expect(result.current.scrollLeft).toBe(500 + EXTENSION_DAYS.week * DAY_WIDTHS.week);
  });

  it("ignores an unknown zoom", () => {
    const { result } = renderHook(() => useGanttViewport(TODAY, 6));
    act(() => result.current.setZoom("day" as never));
    expect(result.current.zoom).toBe("week");
  });
});

describe("clampScroll", () => {
  it("keeps the offset inside the scrollable range", () => {
    expect(clampScroll(-10, 2000, 800)).toBe(0);
    expect(clampScroll(500, 2000, 800)).toBe(500);
    expect(clampScroll(5000, 2000, 800)).toBe(1200);
    expect(clampScroll(50, 400, 800)).toBe(0);
  });
});

describe("timelineWidth", () => {
  it("multiplies the day count by the zoom's width", () => {
    expect(timelineWidth({ startIso: TODAY, dayCount: 10 }, "week")).toBe(10 * DAY_WIDTHS.week);
  });
});
