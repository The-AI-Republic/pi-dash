// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail record write (parity/specs/pages.md §5.1): the title patch is a
// record PATCH, and its echo updates the cached record and collection.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PageDetail, PageRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toaster = vi.hoisted(() => ({ notifyError: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, jsonResponse, makePageRow } from "../testing.js";
import { pageKeys } from "./keys.js";
import { PAGE_MARK_FAILED_TITLE, PAGE_RENAME_FAILED_TITLE, usePageMark, usePageRename } from "./detailMutations.js";

let status = 200;
let patches: { url: string; body: unknown }[] = [];

beforeEach(() => {
  status = 200;
  patches = [];
  toaster.notifyError.mockReset();
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "PATCH") {
      patches.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
      return jsonResponse(url, status, { id: PAGES_PAGE_ID, name: "Renamed" });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function harness(): { client: QueryClient; wrapper: React.FC<{ children: React.ReactNode }> } {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const detail: PageDetail = { ...makePageRow({ name: "Old name" }), description_html: "" };
  const other = makePageRow({ id: "423e4567-e89b-12d3-a456-426614174004", name: "Other" });
  client.setQueryData(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), detail);
  client.setQueryData<PageRow[]>(pageKeys.list("acme", PAGES_PROJECT_ID), [detail, other]);
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

describe("usePageRename", () => {
  it("patches the record and adopts the new name everywhere", async () => {
    const { client, wrapper } = harness();
    const { result } = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate("Renamed");
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(patches).toHaveLength(1);
    expect(patches[0]?.url).toContain(`/pages/${PAGES_PAGE_ID}/`);
    expect(patches[0]?.body).toEqual({ name: "Renamed" });
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.name).toBe("Renamed");
    const rows = client.getQueryData<PageRow[]>(pageKeys.list("acme", PAGES_PROJECT_ID)) ?? [];
    expect(rows.map((row) => row.name)).toEqual(["Renamed", "Other"]);
  });

  it("reports a failed write through the shared notice surface", async () => {
    status = 400;
    const { client, wrapper } = harness();
    const { result } = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate("Renamed");
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toaster.notifyError).toHaveBeenCalledWith(PAGE_RENAME_FAILED_TITLE);
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.name).toBe("Old name");
  });

  it("leaves absent caches absent on success", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate("Renamed");
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.getQueryData(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID))).toBeUndefined();
    expect(client.getQueryData(pageKeys.list("acme", PAGES_PROJECT_ID))).toBeUndefined();
  });
});

describe("usePageMark", () => {
  const MARK = { in_use: "emoji", emoji: { value: "128640" } };

  function markHarness(): { client: QueryClient; wrapper: React.FC<{ children: React.ReactNode }> } {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const detail: PageDetail = {
      ...makePageRow({ name: "Launch notes", logo_props: {} }),
      description_html: "",
    };
    const other = makePageRow({ id: "423e4567-e89b-12d3-a456-426614174004", name: "Other", logo_props: {} });
    client.setQueryData(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), detail);
    client.setQueryData<PageRow[]>(pageKeys.list("acme", PAGES_PROJECT_ID), [detail, other]);
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    return { client, wrapper };
  }

  it("patches only the mark and adopts it in the record and the collection", async () => {
    const { client, wrapper } = markHarness();
    const { result } = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate(MARK);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(patches).toHaveLength(1);
    expect(patches[0]?.url).toContain(`/pages/${PAGES_PAGE_ID}/`);
    expect(patches[0]?.body).toEqual({ logo_props: MARK });
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.logo_props).toEqual(MARK);
    expect(detail?.name).toBe("Launch notes");
    const rows = client.getQueryData<PageRow[]>(pageKeys.list("acme", PAGES_PROJECT_ID)) ?? [];
    expect(rows.map((row) => row.logo_props)).toEqual([MARK, {}]);
  });

  it("clears the mark with an empty record, never null (NEWFRONT-355)", async () => {
    const { client, wrapper } = markHarness();
    const { result } = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate(MARK);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    result.current.mutate({});
    await waitFor(() => expect(patches).toHaveLength(2));
    expect(patches[1]?.body).toEqual({ logo_props: {} });
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.logo_props).toEqual({});
  });

  it("does not mutate the caller's draft record", async () => {
    const { wrapper } = markHarness();
    const { result } = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    const draft = { ...MARK };
    result.current.mutate(draft);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(draft).toEqual(MARK);
  });

  it("reports a failed write through the shared notice surface", async () => {
    status = 400;
    const { client, wrapper } = markHarness();
    const { result } = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate(MARK);
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toaster.notifyError).toHaveBeenCalledWith(PAGE_MARK_FAILED_TITLE);
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.logo_props).toEqual({});
  });

  it("leaves absent caches absent on success", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate(MARK);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.getQueryData(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID))).toBeUndefined();
    expect(client.getQueryData(pageKeys.list("acme", PAGES_PROJECT_ID))).toBeUndefined();
  });
});
