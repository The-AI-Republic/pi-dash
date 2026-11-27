// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail record write (parity/specs/pages.md §5.1): the title patch is a
// record PATCH, and its echo updates the cached record and collection.
// Title saves are serialised (NEWFRONT-358): a burst can never leave two
// renames of one page on the wire, and a superseded name touches nothing.

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
/** The page an operator opens after the one under test. */
const OTHER_PAGE_ID = "423e4567-e89b-12d3-a456-426614174004";
/** When set, each rename PATCH waits for this test to release it. */
let gate: { open: Array<() => void>; count: () => number } | null = null;

beforeEach(() => {
  status = 200;
  patches = [];
  gate = null;
  toaster.notifyError.mockReset();
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "PATCH") {
      patches.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
      if (gate !== null) await new Promise<void>((resolve) => gate?.open.push(resolve));
      return jsonResponse(url, status, { id: PAGES_PAGE_ID, name: "Renamed" });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

/** Hold every rename PATCH open until the test releases it, in order. */
function holdPatches(): void {
  const open: Array<() => void> = [];
  gate = { open, count: () => open.length };
}

/** Release the oldest held PATCH; it settles only after the transport answers. */
async function releaseOldest(): Promise<void> {
  const release = gate?.open.shift();
  if (release === undefined) throw new Error("no held PATCH to release");
  release();
  // Let the transport answer and the queue start the next name.
  await new Promise((resolve) => setTimeout(resolve, 0));
}

afterEach(() => {
  resetAppClient();
});

function harness(): { client: QueryClient; wrapper: React.FC<{ children: React.ReactNode }> } {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const detail: PageDetail = { ...makePageRow({ name: "Old name" }), description_html: "" };
  const other = makePageRow({ id: OTHER_PAGE_ID, name: "Other" });
  client.setQueryData(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), detail);
  client.setQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, OTHER_PAGE_ID), {
    ...other,
    description_html: "",
  });
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

  it("serialises a burst: one rename on the wire, the newest name last (NEWFRONT-358)", async () => {
    holdPatches();
    const { client, wrapper } = harness();
    const { result } = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate("Half");
    await vi.waitFor(() => expect(patches.map((patch) => patch.body)).toEqual([{ name: "Half" }]));
    // Two more names arrive while "Half" is still unanswered: only the last
    // one waits; the middle name is dropped unsent.
    result.current.mutate("Half typ");
    result.current.mutate("Half typed title");
    expect(gate?.count()).toBe(1);
    await releaseOldest();
    await vi.waitFor(() =>
      expect(patches.map((patch) => patch.body)).toEqual([{ name: "Half" }, { name: "Half typed title" }])
    );
    expect(gate?.count()).toBe(1);
    await releaseOldest();
    await vi.waitFor(() => expect(result.current.isSuccess).toBe(true));
    const detail = client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID));
    expect(detail?.name).toBe("Half typed title");
    // The superseded prefix never reached a cache and raised no notice.
    expect(toaster.notifyError).not.toHaveBeenCalled();
  });

  it("reports only the final refusal of a burst (NEWFRONT-358)", async () => {
    holdPatches();
    const { client, wrapper } = harness();
    const { result } = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    result.current.mutate("A");
    await vi.waitFor(() => expect(patches).toHaveLength(1));
    result.current.mutate("AB");
    status = 400;
    await releaseOldest();
    await vi.waitFor(() => expect(patches).toHaveLength(2));
    await releaseOldest();
    await vi.waitFor(() => expect(result.current.isError).toBe(true));
    expect(toaster.notifyError).toHaveBeenCalledTimes(1);
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

  it("sends the page being left's waiting name after a rename on the next page (NEWFRONT-358)", async () => {
    holdPatches();
    const { wrapper } = harness();
    // The detail route keeps one screen across `$pageId` changes, so the
    // hook is re-rendered with the next page rather than remounted.
    const { result, rerender } = renderHook(
      ({ pageId }: { pageId: string }) => usePageRename("acme", PAGES_PROJECT_ID, pageId),
      {
        wrapper,
        initialProps: { pageId: PAGES_PAGE_ID },
      }
    );
    // The title's commit path holds this screen's `mutate`; the blur that
    // leaves A fires it before the route hands the screen page B.
    const saveA = result.current.mutate;
    saveA("A1");
    await vi.waitFor(() => expect(patches.map((patch) => patch.body)).toEqual([{ name: "A1" }]));
    // Leaving A commits its final name, which waits behind "A1".
    saveA("A final");
    rerender({ pageId: OTHER_PAGE_ID });
    // B's rename must not supersede A's waiting name: each page has its own
    // channel, so B writes beside A's one in-flight rename.
    result.current.mutate("B1");
    await vi.waitFor(() => expect(patches.map((patch) => patch.body)).toEqual([{ name: "A1" }, { name: "B1" }]));
    expect(patches[1]?.url).toContain(`/pages/${OTHER_PAGE_ID}/`);
    // Neither page has two renames on the wire at once.
    expect(gate?.count()).toBe(2);
    await releaseOldest();
    // A's final name is still sent, to A, once A's earlier write settled.
    await vi.waitFor(() => expect(patches.map((patch) => patch.body)).toContainEqual({ name: "A final" }));
    const finalOnA = patches.findIndex((patch) => (patch.body as { name: string }).name === "A final");
    expect(patches[finalOnA]?.url).toContain(`/pages/${PAGES_PAGE_ID}/`);
    // Still only A's final name on A and B1 on B: one per page.
    expect(gate?.count()).toBe(2);
    await releaseOldest();
    await releaseOldest();
    expect(toaster.notifyError).not.toHaveBeenCalled();
  });

  it("patches the renamed page's caches after the screen moved to another page (NEWFRONT-358)", async () => {
    holdPatches();
    const { client, wrapper } = harness();
    const { result, rerender } = renderHook(
      ({ pageId }: { pageId: string }) => usePageRename("acme", PAGES_PROJECT_ID, pageId),
      {
        wrapper,
        initialProps: { pageId: PAGES_PAGE_ID },
      }
    );
    const saveA = result.current.mutate;
    saveA("Renamed A");
    await vi.waitFor(() => expect(patches).toHaveLength(1));
    rerender({ pageId: OTHER_PAGE_ID });
    await releaseOldest();
    await waitFor(() =>
      expect(client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID))?.name).toBe(
        "Renamed A"
      )
    );
    expect(client.getQueryData<PageDetail>(pageKeys.detail("acme", PAGES_PROJECT_ID, OTHER_PAGE_ID))?.name).toBe(
      "Other"
    );
    const rows = client.getQueryData<PageRow[]>(pageKeys.list("acme", PAGES_PROJECT_ID)) ?? [];
    expect(rows.map((row) => row.name)).toEqual(["Renamed A", "Other"]);
    expect(toaster.notifyError).not.toHaveBeenCalled();
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

describe("record writes stay outside the description path (PAGE-064)", () => {
  it("title and mark saves patch the record endpoint only", async () => {
    const { wrapper } = harness();
    const rename = renderHook(() => usePageRename("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    rename.result.current.mutate("Renamed");
    await waitFor(() => expect(rename.result.current.isSuccess).toBe(true));
    const mark = renderHook(() => usePageMark("acme", PAGES_PROJECT_ID, PAGES_PAGE_ID), { wrapper });
    mark.result.current.mutate({ in_use: "emoji", emoji: { value: "128640" } });
    await waitFor(() => expect(mark.result.current.isSuccess).toBe(true));
    expect(patches).toHaveLength(2);
    for (const patch of patches) {
      expect(patch.url).toContain(`/pages/${PAGES_PAGE_ID}/`);
      expect(patch.url).not.toContain("/description/");
    }
  });
});
