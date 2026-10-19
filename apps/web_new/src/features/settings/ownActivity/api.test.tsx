// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history read (parity/specs/profile.md §7, PROF-019): one hundred
// rows per page, a cursor only after the first page, and a continuation
// only while the server reports another page.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import {
  nextOwnActivityCursor,
  OWN_ACTIVITY_FIRST_CURSOR,
  OWN_ACTIVITY_PAGE_SIZE,
  ownActivityKeys,
  ownActivityPagesQueryOptions,
  useOwnActivityPages,
} from "./api.js";

function page(cursor: string, next: string | null) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: next ?? "",
    prev_cursor: "",
    next_page_results: next !== null,
    prev_page_results: false,
    count: 0,
    total_pages: next === null ? 1 : 2,
    total_results: 0,
    extra_stats: null,
    results: [],
  };
}

function harness() {
  const urls: URL[] = [];
  const transport = async (url: string) => {
    const parsed = new URL(url);
    urls.push(parsed);
    const cursor = parsed.searchParams.get("cursor");
    const body = cursor === null ? page("", "100:0:0") : page(cursor, null);
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  }
  return { urls, queryClient, wrapper };
}

afterEach(() => resetAppClient());

describe("own activity contract", () => {
  it("pages one hundred rows at a time", () => {
    expect(OWN_ACTIVITY_PAGE_SIZE).toBe(100);
    expect(OWN_ACTIVITY_FIRST_CURSOR).toBe("");
    expect(ownActivityKeys.pages).toEqual(["profile", "own-activity", "pages"]);
  });

  it("continues only with a flagged, non-empty cursor", () => {
    expect(nextOwnActivityCursor(page("", "100:1:0") as never)).toBe("100:1:0");
    expect(nextOwnActivityCursor(page("", null) as never)).toBeUndefined();
    expect(
      nextOwnActivityCursor({ ...page("", null), next_page_results: true, next_cursor: "" } as never)
    ).toBeUndefined();
  });

  it("reads the first page without a cursor and the next one with it", async () => {
    const { urls, wrapper } = harness();
    const { result } = renderHook(() => useOwnActivityPages(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(urls[0]?.pathname).toBe("/api/users/me/activities/");
    expect(urls[0]?.searchParams.get("per_page")).toBe("100");
    expect(urls[0]?.searchParams.has("cursor")).toBe(false);
    expect(result.current.hasNextPage).toBe(true);
    await result.current.fetchNextPage();
    await waitFor(() => expect(result.current.isFetchingNextPage).toBe(false));
    expect(urls[1]?.searchParams.get("cursor")).toBe("100:0:0");
    await waitFor(() => expect(result.current.hasNextPage).toBe(false));
  });

  it("exposes the query options for prefetching", () => {
    const options = ownActivityPagesQueryOptions({} as never);
    expect(options.queryKey).toEqual(ownActivityKeys.pages);
    expect(options.initialPageParam).toBe(OWN_ACTIVITY_FIRST_CURSOR);
    expect(
      options.getNextPageParam({ next_cursor: "", next_page_results: false } as never, [], "", [])
    ).toBeUndefined();
  });
});
