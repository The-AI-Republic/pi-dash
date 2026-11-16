// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one pages collection read (parity/specs/pages.md §3.9, PAGE-020):
// one shared cache entry per workspace and project that asks for shelved
// rows too, and revalidates once each time the list mounts again so a
// return from a page refreshes the held rows without a tab refetch.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makePageRow } from "../testing.js";
import { pageKeys } from "./keys.js";
import { pagesListQueryOptions } from "./queries.js";

import { listPages } from "@pidash/api-client";

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listPages: vi.fn(),
}));

const listPagesMock = vi.mocked(listPages);

function fakeClient(): ApiClient {
  return {} as ApiClient;
}

describe("pagesListQueryOptions", () => {
  it("keys the collection by workspace and project and includes shelved rows", async () => {
    const options = pagesListQueryOptions(fakeClient(), "acme", "project");
    expect(options.queryKey).toEqual(pageKeys.list("acme", "project"));
    listPagesMock.mockResolvedValueOnce([makePageRow({ name: "Shelf" })]);
    await options.queryFn?.({} as never);
    expect(listPagesMock).toHaveBeenCalledWith(expect.anything(), "acme", "project", { includeArchived: true });
  });

  it("revalidates on mount so a return to the list refreshes the held rows", () => {
    expect(pagesListQueryOptions(fakeClient(), "acme", "project").refetchOnMount).toBe("always");
  });

  it("refetches a cached collection each time the list mounts again", async () => {
    // The list unmounts when a row opens and mounts again on the way back;
    // with the entry still fresh that second mount is what refreshes it.
    listPagesMock.mockResolvedValue([makePageRow({ name: "Row" })]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = pagesListQueryOptions(fakeClient(), "acme", "project");
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client }, children);
    const { useQuery } = await import("@tanstack/react-query");
    const first = renderHook(() => useQuery(options), { wrapper });
    await waitFor(() => expect(first.result.current.data).toBeDefined());
    const afterFirstMount = listPagesMock.mock.calls.length;
    first.unmount();
    const second = renderHook(() => useQuery(options), { wrapper });
    await waitFor(() => expect(listPagesMock.mock.calls.length).toBe(afterFirstMount + 1));
    // The rows stay up while the mount read flies: the cache is never
    // dropped, so a return shows content immediately.
    expect(second.result.current.data).toBeDefined();
    second.unmount();
    client.clear();
  });
});
