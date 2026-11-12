// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, keepPreviousData } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { EXPORT_HISTORY_POLL_MS } from "../exportHistory.js";
import { exportsKeys } from "./keys.js";
import { exportsHistoryPageQueryOptions, useExportsHistoryPage } from "./history.js";

interface HistoryRecord {
  id: string;
  created_at: string;
  updated_at: string;
  project: string[] | null;
  provider: string;
  status: string;
  url: string | null;
  initiated_by: string;
  initiated_by_detail: Record<string, unknown>;
  token: string;
  created_by: string | null;
  updated_by: string | null;
}

function historyRecord(overrides: Partial<HistoryRecord> = {}): HistoryRecord {
  return {
    id: "22222222-2222-4222-8222-222222222222",
    created_at: "2024-04-11T12:00:00.000Z",
    updated_at: "2024-04-11T12:00:00.000Z",
    project: ["33333333-3333-4333-8333-333333333333"],
    provider: "csv",
    status: "completed",
    url: "https://files.test/export.csv",
    initiated_by: "11111111-1111-4111-8111-111111111111",
    initiated_by_detail: {
      id: "11111111-1111-4111-8111-111111111111",
      first_name: "Ana",
      last_name: "Siva",
      avatar: "",
      avatar_url: null,
      display_name: "Ana Siva",
      is_bot: false,
    },
    token: "token",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function historyPage(results: HistoryRecord[]) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "10:1:0",
    prev_cursor: "10:-1:1",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_pages: 1,
    total_results: results.length,
    extra_stats: null,
    results,
  };
}

/** Stub transport answering the history read; returns the observed URLs. */
function stubHistoryClient(answer: () => unknown): string[] {
  const urls: string[] = [];
  createAppClient("https://api.test", (async (url: string) => {
    urls.push(url);
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(answer()),
    };
  }) as never);
  return urls;
}

function Probe({ slug, cursor }: { slug: string; cursor: string }): React.ReactElement {
  const page = useExportsHistoryPage(slug, cursor);
  return <p data-testid="rows">{page.data === undefined ? "loading" : String(page.data.results.length)}</p>;
}

function wrapper(client: QueryClient): ({ children }: { children: React.ReactNode }) => React.ReactElement {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

afterEach(() => {
  vi.useRealTimers();
  resetAppClient();
});

describe("exportsHistoryPageQueryOptions", () => {
  it("keys one window under the shared history prefix", () => {
    stubHistoryClient(() => historyPage([]));
    const options = exportsHistoryPageQueryOptions(getAppClient(), "acme", "10:0:0");
    expect(options.queryKey).toEqual([...exportsKeys.history("acme"), "10:0:0"]);
  });

  it("reads with the page size and cursor the server requires", async () => {
    const urls = stubHistoryClient(() => historyPage([historyRecord()]));
    const options = exportsHistoryPageQueryOptions(getAppClient(), "acme", "10:0:0");
    const queryFn = options.queryFn;
    expect(queryFn).toBeDefined();
    if (!queryFn) return;
    const page = await queryFn({} as never);
    expect(page.results).toHaveLength(1);
    expect(urls).toEqual(["https://api.test/api/workspaces/acme/export-issues?per_page=10&cursor=10%3A0%3A0"]);
  });

  it("pages over stale rows and never retries behind the inline error", () => {
    stubHistoryClient(() => historyPage([]));
    const options = exportsHistoryPageQueryOptions(getAppClient(), "acme", "10:0:0");
    expect(options.placeholderData).toBe(keepPreviousData);
    expect(options.retry).toBe(false);
  });

  it("polls while a mounted row processes and idles once settled", () => {
    stubHistoryClient(() => historyPage([]));
    const options = exportsHistoryPageQueryOptions(getAppClient(), "acme", "10:0:0");
    const cadence = options.refetchInterval;
    expect(typeof cadence).toBe("function");
    if (typeof cadence !== "function") return;
    expect(cadence({ state: { data: undefined } } as never)).toBe(false);
    expect(cadence({ state: { data: historyPage([historyRecord({ status: "processing" })]) } } as never)).toBe(
      EXPORT_HISTORY_POLL_MS
    );
    expect(cadence({ state: { data: historyPage([historyRecord({ status: "completed" })]) } } as never)).toBe(false);
  });
});

describe("useExportsHistoryPage", () => {
  it("re-reads the mounted window when the history prefix is invalidated", async () => {
    const urls = stubHistoryClient(() => historyPage([historyRecord()]));
    const client = new QueryClient();
    render(<Probe slug="acme" cursor="10:0:0" />, { wrapper: wrapper(client) });
    await waitFor(() => expect(screen.getByTestId("rows")).toHaveTextContent("1"));
    expect(urls).toHaveLength(1);

    await act(async () => {
      await client.invalidateQueries({ queryKey: exportsKeys.history("acme") });
    });
    await waitFor(() => expect(urls).toHaveLength(2));
  });

  it("re-reads while a row processes and settles once terminal", async () => {
    vi.useFakeTimers();
    let status = "processing";
    const urls = stubHistoryClient(() => historyPage([historyRecord({ status })]));
    const client = new QueryClient();
    render(<Probe slug="acme" cursor="10:0:0" />, { wrapper: wrapper(client) });
    await vi.waitFor(() => expect(screen.getByTestId("rows")).toHaveTextContent("1"));
    expect(urls).toHaveLength(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(EXPORT_HISTORY_POLL_MS);
    });
    expect(urls).toHaveLength(2);

    status = "completed";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(EXPORT_HISTORY_POLL_MS);
    });
    expect(urls).toHaveLength(3);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(EXPORT_HISTORY_POLL_MS * 3);
    });
    expect(urls).toHaveLength(3);
  });

  it("never polls a settled first window", async () => {
    vi.useFakeTimers();
    const urls = stubHistoryClient(() => historyPage([historyRecord({ status: "failed" })]));
    const client = new QueryClient();
    render(<Probe slug="acme" cursor="10:0:0" />, { wrapper: wrapper(client) });
    await vi.waitFor(() => expect(screen.getByTestId("rows")).toHaveTextContent("1"));
    expect(urls).toHaveLength(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(EXPORT_HISTORY_POLL_MS * 3);
    });
    expect(urls).toHaveLength(1);
  });
});
