// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Realtime error channel (parity/specs/pages.md §8.6, PAGE-069, PAGE-063):
// every decoded error reaches the subscribers, a too-large report raises
// the strip's incident once per incident with an optional dismiss, and a
// direct too-large report (a 413 fallback patch) raises the same latch.

import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  PAGE_CONTENT_TOO_LARGE,
  dismissPageOversizedIncident,
  reportPageContentTooLarge,
  reportPageRealtimeError,
  resetPageRealtimeErrors,
  subscribePageRealtimeErrors,
  usePageOversizedIncident,
} from "./pageRealtimeErrors.js";

afterEach(() => {
  resetPageRealtimeErrors();
});

describe("reportPageRealtimeError", () => {
  it("dispatches every error to every subscriber until unsubscribed", () => {
    const first = vi.fn();
    const second = vi.fn();
    const unsubscribe = subscribePageRealtimeErrors(first);
    subscribePageRealtimeErrors(second);
    const error = {
      pageId: "page-1",
      phase: "persist" as const,
      code: "page_locked",
      message: "locked",
      shouldDisconnect: false,
    };
    act(() => {
      reportPageRealtimeError(error);
    });
    expect(first).toHaveBeenCalledWith(error);
    expect(second).toHaveBeenCalledWith(error);
    unsubscribe();
    act(() => {
      reportPageRealtimeError(error);
    });
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);
  });

  it("raises the oversized incident only for the too-large code", () => {
    const { result } = renderHook(() => usePageOversizedIncident("page-1"));
    expect(result.current.visible).toBe(false);
    act(() => {
      reportPageRealtimeError({
        pageId: "page-1",
        phase: "persist",
        code: "page_locked",
        message: "locked",
        shouldDisconnect: false,
      });
    });
    expect(result.current.visible).toBe(false);
    act(() => {
      reportPageRealtimeError({
        pageId: "page-1",
        phase: "persist",
        code: PAGE_CONTENT_TOO_LARGE,
        message: "too large",
        shouldDisconnect: true,
      });
    });
    expect(result.current.visible).toBe(true);
  });
});

describe("usePageOversizedIncident", () => {
  it("ignores incidents raised for other pages", () => {
    const { result } = renderHook(() => usePageOversizedIncident("page-1"));
    act(() => {
      reportPageContentTooLarge("page-2");
    });
    expect(result.current.visible).toBe(false);
    act(() => {
      reportPageContentTooLarge("page-1");
    });
    expect(result.current.visible).toBe(true);
  });

  it("stays fired once while visible and re-fires after a dismiss", () => {
    const { result } = renderHook(() => usePageOversizedIncident("page-1"));
    act(() => {
      reportPageContentTooLarge("page-1");
      reportPageContentTooLarge("page-1");
    });
    expect(result.current.visible).toBe(true);
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.visible).toBe(false);
    act(() => {
      reportPageContentTooLarge("page-1");
    });
    expect(result.current.visible).toBe(true);
  });

  it("dismisses through the module function as well", () => {
    const { result } = renderHook(() => usePageOversizedIncident("page-1"));
    act(() => {
      reportPageContentTooLarge("page-1");
    });
    expect(result.current.visible).toBe(true);
    act(() => {
      dismissPageOversizedIncident("page-1");
    });
    expect(result.current.visible).toBe(false);
  });

  it("a dismiss with no incident keeps the strip hidden", () => {
    const { result } = renderHook(() => usePageOversizedIncident("page-1"));
    act(() => {
      dismissPageOversizedIncident("page-1");
    });
    expect(result.current.visible).toBe(false);
  });
});
