// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  createCatalogPager,
  currentPagerCursor,
  nextCatalogPage,
  pagerPage,
  previousCatalogPage,
  useCatalogPager,
} from "./lib/pager.js";

describe("catalog pager stack", () => {
  it("opens on the first window without a cursor", () => {
    const pager = createCatalogPager("");
    expect(currentPagerCursor(pager)).toBeNull();
    expect(pagerPage(pager)).toBe(1);
  });

  it("steps onto later windows over their cursors", () => {
    const first = createCatalogPager("");
    const second = nextCatalogPage(first, "cursor-2");
    expect(currentPagerCursor(second)).toBe("cursor-2");
    expect(pagerPage(second)).toBe(2);
    const third = nextCatalogPage(second, "cursor-3");
    expect(currentPagerCursor(third)).toBe("cursor-3");
    expect(pagerPage(third)).toBe(3);
  });

  it("stays put without a next cursor", () => {
    const pager = createCatalogPager("");
    expect(nextCatalogPage(pager, null)).toBe(pager);
    expect(nextCatalogPage(pager, undefined)).toBe(pager);
  });

  it("reads an out-of-range window as the first", () => {
    expect(currentPagerCursor({ query: "", cursors: [null], at: 3 })).toBeNull();
  });

  it("steps back through visited windows, re-reading the first cursorless", () => {
    const second = nextCatalogPage(createCatalogPager(""), "cursor-2");
    const back = previousCatalogPage(second);
    expect(currentPagerCursor(back)).toBeNull();
    expect(pagerPage(back)).toBe(1);
    expect(previousCatalogPage(back)).toBe(back);
  });

  it("drops the stale tail when stepping forward past a visited page", () => {
    const third = nextCatalogPage(nextCatalogPage(createCatalogPager(""), "cursor-2"), "cursor-3");
    const back = previousCatalogPage(third);
    const fork = nextCatalogPage(back, "cursor-2b");
    expect(currentPagerCursor(fork)).toBe("cursor-2b");
    expect(fork.cursors).toEqual([null, "cursor-2", "cursor-2b"]);
  });
});

describe("useCatalogPager", () => {
  it("pages through windows and back", () => {
    const { result } = renderHook(() => useCatalogPager(""));
    expect(result.current.cursor).toBeNull();
    expect(result.current.page).toBe(1);
    act(() => {
      result.current.goNext("cursor-2");
    });
    expect(result.current.cursor).toBe("cursor-2");
    expect(result.current.page).toBe(2);
    act(() => {
      result.current.goPrevious();
    });
    expect(result.current.cursor).toBeNull();
    expect(result.current.page).toBe(1);
  });

  it("ignores steps with no window behind them", () => {
    const { result } = renderHook(() => useCatalogPager(""));
    act(() => {
      result.current.goNext(null);
      result.current.goPrevious();
    });
    expect(result.current.cursor).toBeNull();
    expect(result.current.page).toBe(1);
  });

  it("rewinds the stack only when the query actually changes (CLOUD-023)", () => {
    const { result, rerender } = renderHook(({ query }: { query: string }) => useCatalogPager(query), {
      initialProps: { query: "" },
    });
    act(() => {
      result.current.goNext("cursor-2");
    });
    expect(result.current.page).toBe(2);
    rerender({ query: "" });
    expect(result.current.cursor).toBe("cursor-2");
    expect(result.current.page).toBe(2);
    rerender({ query: "notes" });
    expect(result.current.cursor).toBeNull();
    expect(result.current.page).toBe(1);
  });
});
