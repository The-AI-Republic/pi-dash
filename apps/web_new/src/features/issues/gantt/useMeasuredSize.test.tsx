// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Element measurement (parity/specs/issues.md §6, ISS-046/048). The hook
// reports the border-box size on mount, follows the ResizeObserver, skips
// a no-op report, survives an observer entry without a box, and falls
// back to a one-shot read on engines without ResizeObserver.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useMeasuredSize } from "./useMeasuredSize.js";

type ResizeCallback = (entries: Array<{ contentRect: { width: number; height: number } }>) => void;

interface ObserverStub {
  callback: ResizeCallback | null;
  observed: Element[];
  disconnected: number;
}

function stubObserver(): ObserverStub {
  const stub: ObserverStub = { callback: null, observed: [], disconnected: 0 };
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeCallback) {
        stub.callback = callback;
      }
      observe(element: Element): void {
        stub.observed.push(element);
      }
      unobserve(): void {}
      disconnect(): void {
        stub.disconnected += 1;
      }
    }
  );
  return stub;
}

function elementOf(width: number, height: number): HTMLDivElement {
  const element = document.createElement("div");
  Object.defineProperty(element, "clientWidth", { configurable: true, value: width });
  Object.defineProperty(element, "clientHeight", { configurable: true, value: height });
  document.body.append(element);
  return element;
}

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

describe("useMeasuredSize", () => {
  it("reports the element's client size on mount", () => {
    stubObserver();
    const element = elementOf(900, 400);
    const { result } = renderHook(() => useMeasuredSize(element));
    expect(result.current).toEqual({ width: 900, height: 400 });
  });

  it("follows later observer entries and ignores an empty one", () => {
    const stub = stubObserver();
    const element = elementOf(900, 400);
    const { result } = renderHook(() => useMeasuredSize(element));
    expect(stub.observed).toContain(element);
    act(() => stub.callback?.([{ contentRect: { width: 1200, height: 500 } }]));
    expect(result.current).toEqual({ width: 1200, height: 500 });
    act(() => stub.callback?.([]));
    expect(result.current).toEqual({ width: 1200, height: 500 });
  });

  it("does not re-render for an identical report", () => {
    const stub = stubObserver();
    const element = elementOf(900, 400);
    const { result } = renderHook(() => useMeasuredSize(element));
    const before = result.current;
    act(() => stub.callback?.([{ contentRect: { width: 900, height: 400 } }]));
    expect(result.current).toBe(before);
  });

  it("disconnects the observer on unmount", () => {
    const stub = stubObserver();
    const element = elementOf(900, 400);
    const { unmount } = renderHook(() => useMeasuredSize(element));
    unmount();
    expect(stub.disconnected).toBeGreaterThanOrEqual(1);
  });

  it("reports zero while there is no element", () => {
    stubObserver();
    const { result } = renderHook(() => useMeasuredSize(null));
    expect(result.current).toEqual({ width: 0, height: 0 });
  });

  it("falls back to a one-shot read without ResizeObserver", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    const element = elementOf(640, 320);
    const { result } = renderHook(() => useMeasuredSize(element));
    expect(result.current).toEqual({ width: 640, height: 320 });
  });

  it("re-attaches when the watched ref changes", () => {
    const stub = stubObserver();
    const first = elementOf(300, 200);
    const second = elementOf(800, 600);
    const { result, rerender } = renderHook(({ element }: { element: HTMLDivElement }) => useMeasuredSize(element), {
      initialProps: { element: first },
    });
    expect(result.current).toEqual({ width: 300, height: 200 });
    rerender({ element: second });
    expect(result.current).toEqual({ width: 800, height: 600 });
    expect(stub.disconnected).toBeGreaterThanOrEqual(1);
    expect(stub.observed).toContain(first);
    expect(stub.observed).toContain(second);
  });
});
