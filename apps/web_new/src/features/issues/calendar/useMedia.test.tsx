// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useNarrowViewport, useTouchInput } from "./useMedia.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("calendar media helpers", () => {
  it("falls back to desktop pointer behaviour without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    const narrow = renderHook(() => useNarrowViewport());
    const touch = renderHook(() => useTouchInput());
    expect(narrow.result.current).toBe(false);
    expect(touch.result.current).toBe(false);
  });

  it("reads and subscribes to the narrow viewport query", async () => {
    const listeners: Array<() => void> = [];
    const query = {
      matches: false,
      media: "(max-width: 767px)",
      addEventListener: vi.fn((_event: string, next: () => void) => {
        listeners.push(next);
      }),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", () => query);
    const { result, unmount } = renderHook(() => useNarrowViewport());
    expect(result.current).toBe(false);
    query.matches = true;
    listeners[0]?.();
    await waitFor(() => expect(result.current).toBe(true));
    unmount();
    expect(query.removeEventListener).toHaveBeenCalled();
  });

  it("detects coarse pointers and touch points", () => {
    vi.stubGlobal("matchMedia", (value: string) => ({ matches: value.includes("coarse") }));
    expect(renderHook(() => useTouchInput()).result.current).toBe(true);
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    vi.stubGlobal("navigator", { maxTouchPoints: 1 });
    expect(renderHook(() => useTouchInput()).result.current).toBe(true);
  });
});
