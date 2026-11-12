// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useBackgroundFetchGauge } from "./useBackgroundFetch.js";

describe("useBackgroundFetchGauge (NTF-013)", () => {
  it("counts a started re-read until it finishes", () => {
    const { result } = renderHook(() => useBackgroundFetchGauge());
    expect(result.current.count).toBe(0);
    let finish = () => {};
    act(() => {
      finish = result.current.begin();
    });
    expect(result.current.count).toBe(1);
    act(() => finish());
    expect(result.current.count).toBe(0);
  });

  it("counts concurrent re-reads and finishes each once", () => {
    const { result } = renderHook(() => useBackgroundFetchGauge());
    let first = () => {};
    let second = () => {};
    act(() => {
      first = result.current.begin();
      second = result.current.begin();
    });
    expect(result.current.count).toBe(2);
    act(() => {
      first();
      first();
    });
    expect(result.current.count).toBe(1);
    act(() => second());
    expect(result.current.count).toBe(0);
  });

  it("finishes a second re-read independently of the first", () => {
    const { result } = renderHook(() => useBackgroundFetchGauge());
    let first = () => {};
    let second = () => {};
    act(() => {
      first = result.current.begin();
      second = result.current.begin();
    });
    act(() => second());
    expect(result.current.count).toBe(1);
    act(() => first());
    expect(result.current.count).toBe(0);
  });
});
