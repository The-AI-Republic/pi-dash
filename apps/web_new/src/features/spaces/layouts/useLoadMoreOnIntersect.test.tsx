// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useLoadMoreOnIntersect } from "./useLoadMoreOnIntersect.js";

interface ObserverRecord {
  callback: IntersectionObserverCallback;
  options: IntersectionObserverInit | undefined;
  observed: Element[];
  disconnected: boolean;
}

const observers: ObserverRecord[] = [];

class TestIntersectionObserver {
  constructor(
    private readonly callback: IntersectionObserverCallback,
    private readonly options?: IntersectionObserverInit
  ) {}
  observe(element: Element): void {
    observers.push({ callback: this.callback, options: this.options, observed: [element], disconnected: false });
  }
  disconnect(): void {
    const record = observers.at(-1);
    if (record !== undefined) record.disconnected = true;
  }
}

function Probe({
  root,
  enabled,
  onLoadMore,
}: {
  root: Element | null;
  enabled: boolean;
  onLoadMore: () => void;
}): React.ReactElement {
  const sentinel = useLoadMoreOnIntersect({ root, enabled, onLoadMore });
  return <div data-testid="sentinel" ref={sentinel} />;
}

function intersect(isIntersecting: boolean, index = observers.length - 1): void {
  const record = observers[index];
  if (record === undefined) throw new Error("no observer installed");
  record.callback([{ isIntersecting } as IntersectionObserverEntry], {} as IntersectionObserver);
}

afterEach(() => {
  vi.unstubAllGlobals();
  observers.length = 0;
});

describe("useLoadMoreOnIntersect (SPC-026)", () => {
  it("attaches to the sentinel within the given root with a lead margin", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    render(<Probe root={null} enabled onLoadMore={() => undefined} />);
    const record = observers[0];
    expect(record).toBeDefined();
    expect(record?.observed[0]).toHaveAttribute("data-testid", "sentinel");
    expect(record?.options).toMatchObject({ root: null, rootMargin: "0px 0px 120px 0px" });
  });

  it("calls back only for an intersecting entry", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    const onLoadMore = vi.fn();
    render(<Probe root={null} enabled onLoadMore={onLoadMore} />);
    intersect(false);
    expect(onLoadMore).not.toHaveBeenCalled();
    intersect(true);
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("does not observe while disabled", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    render(<Probe root={null} enabled={false} onLoadMore={() => undefined} />);
    expect(observers).toHaveLength(0);
  });

  it("does nothing when the environment has no IntersectionObserver", () => {
    expect(() => render(<Probe root={null} enabled onLoadMore={() => undefined} />)).not.toThrow();
    expect(observers).toHaveLength(0);
  });

  it("rebuilds the observer for a new root and disconnects the old one", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    const onLoadMore = vi.fn();
    const { rerender } = render(<Probe root={null} enabled onLoadMore={onLoadMore} />);
    const column = document.createElement("div");
    rerender(<Probe root={column} enabled onLoadMore={onLoadMore} />);
    expect(observers).toHaveLength(2);
    expect(observers[1]?.options?.root).toBe(column);
    // The second observer is the live one; the first one was torn down.
    expect(observers[0]?.disconnected).toBe(true);
    intersect(true, 1);
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("reads the newest callback without rebuilding the observer", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(<Probe root={null} enabled onLoadMore={first} />);
    rerender(<Probe root={null} enabled onLoadMore={second} />);
    expect(observers).toHaveLength(1);
    intersect(true);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("disconnects on unmount", () => {
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    const { unmount } = render(<Probe root={null} enabled onLoadMore={() => undefined} />);
    unmount();
    expect(observers[0]?.disconnected).toBe(true);
  });
});
