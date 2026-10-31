// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Draft data layer (parity/specs/issues.md §12, ISS-137–141): the list
// options, cache math, the four writes and their toasts.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError, type DraftIssue, type DraftIssueListResponse } from "@pidash/api-client";

const toasts = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", () => toasts);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  DRAFT_CREATED_BODY,
  DRAFT_CREATED_TITLE,
  DRAFT_CREATE_FAILED_BODY,
  DRAFT_CREATE_FAILED_TITLE,
  DRAFT_DELETED_BODY,
  DRAFT_DELETED_TITLE,
  DRAFT_PAGE_SIZE,
  DRAFT_PROMOTED_BODY,
  DRAFT_PROMOTED_TITLE,
  DRAFT_UPDATE_FAILED_TITLE,
  createDraftWriteQueue,
  draftKeys,
  draftListQueryOptions,
  draftTotalCount,
  flattenDraftPages,
  isDraftPermissionRefusal,
  mapPagesHolding,
  paintDraftPatch,
  removeDraftFromCache,
  useCreateDraft,
  useDeleteDraft,
  usePromoteDraft,
  useUpdateDraftField,
} from "./api.js";

const WS = "acme";
const DRAFT_1 = "123e4567-e89b-12d3-a456-426614174000";
const DRAFT_2 = "323e4567-e89b-12d3-a456-426614174002";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const USER = "423e4567-e89b-42d3-8456-426614174003";

function draft(id: string, name: string, overrides: Partial<DraftIssue> = {}): DraftIssue {
  return {
    id,
    name,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: USER,
    updated_by: null,
    type_id: null,
    description_html: "",
    ...overrides,
  } as DraftIssue;
}

function page(results: DraftIssue[], overrides: Partial<DraftIssueListResponse> = {}): DraftIssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
    ...overrides,
  };
}

function infinite(
  pages: DraftIssueListResponse[],
  slugs: string[] = []
): { pages: DraftIssueListResponse[]; pageParams: unknown[] } {
  return { pages, pageParams: slugs.length > 0 ? slugs : pages.map(() => "") };
}

/** A transport that answers each request from the handler and records it. */
type Handler = (url: string, method: string, body: unknown) => { status: number; body?: unknown };

function transport(handler: Handler) {
  const calls: Array<{ url: string; method: string; body: unknown; query: unknown }> = [];
  const run = async (requestUrl: string, init: { method?: string; body?: string } = {}) => {
    const method = init.method ?? "GET";
    const sent: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    const parsed = new URL(requestUrl);
    const query = Object.fromEntries(parsed.searchParams.entries());
    const path = `${parsed.origin}${parsed.pathname}`;
    calls.push({ url: path, method, body: sent, query });
    const answer = path.endsWith("/auth/get-csrf-token/")
      ? { status: 200, body: { csrf_token: "csrf" } }
      : handler(path, method, sent);
    return {
      status: answer.status,
      url: requestUrl,
      headers: { get: () => "application/json" },
      text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
    };
  };
  return { calls, run };
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return (
      <QueryClientProvider client={client}>
        <React.Fragment>{children as React.ReactNode}</React.Fragment>
      </QueryClientProvider>
    );
  };
}

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

beforeEach(() => {
  toasts.notifySuccess.mockReset();
  toasts.notifyError.mockReset();
});

afterEach(() => {
  resetAppClient();
});

describe("draftListQueryOptions", () => {
  it("asks the workspace endpoint for fifty rows and no cursor on the first page", async () => {
    const t = transport(() => ({ status: 200, body: page([draft(DRAFT_1, "One")]) }));
    const client = createAppClient("https://api.test", t.run as never);
    const options = draftListQueryOptions(client, WS);
    const first = await options.queryFn!({
      pageParam: "",
      queryKey: options.queryKey,
      signal: new AbortController().signal,
    } as never);
    expect(t.calls[0]).toMatchObject({
      url: "https://api.test/api/workspaces/acme/draft-issues/",
      method: "GET",
      query: { per_page: String(DRAFT_PAGE_SIZE) },
    });
    expect(DRAFT_PAGE_SIZE).toBe(50);
    expect(first.results).toHaveLength(1);
  });

  it("sends the cursor on later pages", async () => {
    const t = transport(() => ({ status: 200, body: page([]) }));
    const client = createAppClient("https://api.test", t.run as never);
    const options = draftListQueryOptions(client, WS);
    await options.queryFn!({
      pageParam: "cursor-2",
      queryKey: options.queryKey,
      signal: new AbortController().signal,
    } as never);
    expect(t.calls[0]?.query).toMatchObject({ per_page: "50", cursor: "cursor-2" });
  });

  it("continues only while the server has another page and a cursor", () => {
    const options = draftListQueryOptions({} as never, WS);
    const next = options.getNextPageParam as (last: DraftIssueListResponse, all: unknown, lastParam: string) => unknown;
    expect(next(page([], { next_page_results: true, next_cursor: "c1" }), [], "")).toBe("c1");
    expect(next(page([], { next_page_results: false, next_cursor: "c1" }), [], "")).toBeUndefined();
    expect(next(page([], { next_page_results: true, next_cursor: "" }), [], "")).toBeUndefined();
  });

  it("keys the list under the workspace scope", () => {
    expect(draftKeys.list(WS).at(-1)).toBe("list");
    expect(draftKeys.list(WS)).toEqual([...draftKeys.all(WS), "list"]);
  });

  it("marks the list owner-scoped so the desktop cache never keeps it", () => {
    // DRAFT-001: the rows are the viewer's own drafts. The desktop bundle
    // keeps its query cache on disk for the next launch, so this list must
    // be excluded from that payload; otherwise the next account on the
    // machine paints the previous account's drafts without a read.
    const options = draftListQueryOptions({} as never, WS);
    expect(options.meta).toEqual({ ownerScoped: true });
  });

  it("reads with the shared list freshness and inherits the app's no-refocus reads", async () => {
    // DRAFT-026: outside changes appear only on a fresh fetch. The app
    // client pins refetchOnWindowFocus/refetchOnReconnect false (ISS-221);
    // the drafts query must not opt back in.
    const { LIST_STALE_TIME_MS } = await import("../../../core/query/client.js");
    const { createQueryClient } = await import("../../../core/query/client.js");
    const options = draftListQueryOptions({} as never, WS);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBeUndefined();
    expect(options.refetchOnReconnect).toBeUndefined();
    const defaults = createQueryClient().getDefaultOptions().queries;
    expect(defaults?.refetchOnWindowFocus).toBe(false);
    expect(defaults?.refetchOnReconnect).toBe(false);
  });
});

describe("page shape", () => {
  it("flattens pages in server order and drops a row repeated across pages", () => {
    const data = infinite([
      page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")]),
      page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Three")]),
    ]);
    expect(flattenDraftPages(data).map((row) => row.name)).toEqual(["One", "Two"]);
    expect(flattenDraftPages(undefined)).toEqual([]);
  });

  it("reads the total from the newest page, falling back to zero", () => {
    expect(draftTotalCount(infinite([page([draft(DRAFT_1, "One")], { total_count: 4 })]))).toBe(4);
    expect(draftTotalCount({ pages: [] })).toBe(0);
    expect(draftTotalCount(undefined)).toBe(0);
  });
});

describe("cache math", () => {
  it("paints a field into the page holding the row and leaves other pages alone", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    client.setQueryData(key, infinite([page([draft(DRAFT_1, "One")]), page([draft(DRAFT_2, "Two")])]));
    paintDraftPatch(client, WS, DRAFT_1, { priority: "high" });
    const next = client.getQueryData<{ pages: DraftIssueListResponse[] }>(key);
    expect(next?.pages[0]?.results[0]?.priority).toBe("high");
    expect(next?.pages[1]).toBe(client.getQueryData<{ pages: DraftIssueListResponse[] }>(key)?.pages[1]);
  });

  it("leaves a row beside the painted one untouched", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    const neighbour = draft(DRAFT_2, "Two");
    client.setQueryData(key, infinite([page([draft(DRAFT_1, "One"), neighbour])]));
    paintDraftPatch(client, WS, DRAFT_1, { priority: "urgent" });
    const next = client.getQueryData<{ pages: DraftIssueListResponse[] }>(key);
    expect(next?.pages[0]?.results[0]?.priority).toBe("urgent");
    expect(next?.pages[0]?.results[1]).toBe(neighbour);
  });

  it("paints a row that lives on a later page too", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    client.setQueryData(key, infinite([page([draft(DRAFT_1, "One")]), page([draft(DRAFT_2, "Two")])]));
    paintDraftPatch(client, WS, DRAFT_2, { name: "Renamed" });
    const next = client.getQueryData<{ pages: DraftIssueListResponse[] }>(key);
    expect(next?.pages[1]?.results[0]?.name).toBe("Renamed");
    expect(next?.pages[0]?.results[0]?.name).toBe("One");
  });

  it("keeps a page that holds the row but changes the list it lives in", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    const first = page([draft(DRAFT_1, "One")]);
    const second = page([draft(DRAFT_2, "Two")]);
    client.setQueryData(key, infinite([first, second]));
    paintDraftPatch(client, WS, DRAFT_1, { priority: "high" });
    const next = client.getQueryData<{ pages: DraftIssueListResponse[] }>(key);
    expect(next?.pages[1]).toBe(second);
    expect(next?.pages[0]).not.toBe(first);
    expect(next?.pages[0]?.results[0]?.priority).toBe("high");
    expect(next?.pages[0]?.results).toHaveLength(1);
  });

  it("leaves an absent cache absent", () => {
    // React Query can hand the updater no data when the key matches a
    // query that has not resolved; the writers must not invent a page.
    expect(mapPagesHolding(undefined, DRAFT_1, () => page([]))).toBeUndefined();
  });

  it("does nothing when the list is not cached", () => {
    const client = newClient();
    expect(() => paintDraftPatch(client, WS, DRAFT_1, { priority: "low" })).not.toThrow();
    expect(() => removeDraftFromCache(client, WS, DRAFT_1)).not.toThrow();
  });

  it("drops a row and lowers each holding page's total, never below zero", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    client.setQueryData(
      key,
      infinite([
        page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")], { total_count: 2 }),
        page([draft(DRAFT_1, "One")], { total_count: 0 }),
      ])
    );
    removeDraftFromCache(client, WS, DRAFT_1);
    const next = client.getQueryData<{ pages: DraftIssueListResponse[] }>(key);
    expect(next?.pages[0]?.results.map((row) => row.id)).toEqual([DRAFT_2]);
    expect(next?.pages[0]?.total_count).toBe(1);
    expect(next?.pages[1]?.total_count).toBe(0);
    expect(next?.pages[1]?.results).toEqual([]);
  });

  it("leaves a page without the row untouched", () => {
    const client = newClient();
    const key = draftKeys.list(WS);
    const kept = page([draft(DRAFT_2, "Two")], { total_count: 1 });
    client.setQueryData(key, infinite([kept]));
    removeDraftFromCache(client, WS, DRAFT_1);
    expect(client.getQueryData<{ pages: DraftIssueListResponse[] }>(key)?.pages[0]).toBe(kept);
  });
});

describe("isDraftPermissionRefusal", () => {
  it("recognises a 403 and the server's creator-or-admin wording", () => {
    expect(isDraftPermissionRefusal(new ApiError({ status: 403, code: "http", message: "Forbidden" }))).toBe(true);
    expect(isDraftPermissionRefusal(new Error("Only admin or creator can delete the work item"))).toBe(true);
    expect(
      isDraftPermissionRefusal(
        new ApiError({ status: 500, code: "http", message: "Only admin or creator can delete the work item" })
      )
    ).toBe(true);
  });

  it("treats other failures as generic", () => {
    expect(isDraftPermissionRefusal(new ApiError({ status: 500, code: "http", message: "Server error" }))).toBe(false);
    expect(isDraftPermissionRefusal(new Error("Network down"))).toBe(false);
    expect(isDraftPermissionRefusal("nope")).toBe(false);
  });
});

describe("createDraftWriteQueue", () => {
  function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<T>((accept, refuse) => {
      resolve = accept;
      reject = refuse;
    });
    return { promise, resolve, reject };
  }

  it("runs one draft's writes in order and answers each task's value", async () => {
    const queue = createDraftWriteQueue();
    const first = deferred<string>();
    const order: string[] = [];
    const one = queue(DRAFT_1, async () => {
      order.push("first-start");
      const value = await first.promise;
      order.push("first-end");
      return value;
    });
    const two = queue(DRAFT_1, async () => {
      order.push("second");
      return "two";
    });
    // The lane holds the second write until the first settles.
    await Promise.resolve();
    await Promise.resolve();
    expect(order).toEqual(["first-start"]);
    first.resolve("one");
    await expect(one).resolves.toBe("one");
    await expect(two).resolves.toBe("two");
    expect(order).toEqual(["first-start", "first-end", "second"]);
  });

  it("runs different drafts' writes side by side", async () => {
    const queue = createDraftWriteQueue();
    const started: string[] = [];
    const one = queue(DRAFT_1, async () => {
      started.push(DRAFT_1);
      return 1;
    });
    const two = queue(DRAFT_2, async () => {
      started.push(DRAFT_2);
      return 2;
    });
    await expect(one).resolves.toBe(1);
    await expect(two).resolves.toBe(2);
    expect(started).toEqual([DRAFT_1, DRAFT_2]);
  });

  it("keeps the lane open after a failure and reports the task's error", async () => {
    const queue = createDraftWriteQueue();
    const failing = queue(DRAFT_1, async () => {
      throw new Error("refused");
    });
    const following = queue(DRAFT_1, async () => "recovered");
    await expect(failing).rejects.toThrow("refused");
    await expect(following).resolves.toBe("recovered");
    // A settled lane accepts new work without waiting on the past.
    await expect(queue(DRAFT_1, async () => "again")).resolves.toBe("again");
    // A lone failure still settles the lane it ran on.
    await expect(
      queue(DRAFT_1, async () => {
        throw new Error("alone");
      })
    ).rejects.toThrow("alone");
    await expect(queue(DRAFT_1, async () => "after")).resolves.toBe("after");
  });
});

describe("writes", () => {
  it("patches one draft and paints it without a toast", async () => {
    const t = transport(() => ({ status: 204 }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([draft(DRAFT_1, "One")])]));
    const { result } = renderHook(() => useUpdateDraftField(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ draftId: DRAFT_1, patch: { priority: "urgent" } });
    });
    const write = t.calls.find((call) => call.method === "PATCH");
    expect(write).toMatchObject({ url: `https://api.test/api/workspaces/acme/draft-issues/${DRAFT_1}/` });
    expect(write?.body).toEqual({ priority: "urgent" });
    expect(
      queryClient.getQueryData<{ pages: DraftIssueListResponse[] }>(draftKeys.list(WS))?.pages[0]?.results[0]?.priority
    ).toBe("urgent");
    expect(toasts.notifySuccess).not.toHaveBeenCalled();
  });

  it("rolls an optimistic patch back and explains a failed field write", async () => {
    const t = transport(() => ({ status: 500, body: { error: "boom" } }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([draft(DRAFT_1, "One")])]));
    const { result } = renderHook(() => useUpdateDraftField(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ draftId: DRAFT_1, patch: { priority: "urgent" } }).catch(() => undefined);
    });
    expect(
      queryClient.getQueryData<{ pages: DraftIssueListResponse[] }>(draftKeys.list(WS))?.pages[0]?.results[0]?.priority
    ).toBe("none");
    expect(toasts.notifyError).toHaveBeenCalledWith(DRAFT_UPDATE_FAILED_TITLE, expect.any(String));
  });

  it("deletes a draft, drops the row and toasts", async () => {
    const t = transport(() => ({ status: 204 }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")])]));
    const { result } = renderHook(() => useDeleteDraft(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync(DRAFT_1);
    });
    expect(t.calls.find((call) => call.method === "DELETE")).toMatchObject({
      url: `https://api.test/api/workspaces/acme/draft-issues/${DRAFT_1}/`,
    });
    expect(toasts.notifySuccess).toHaveBeenCalledWith(DRAFT_DELETED_TITLE, DRAFT_DELETED_BODY);
    expect(
      queryClient
        .getQueryData<{ pages: DraftIssueListResponse[] }>(draftKeys.list(WS))
        ?.pages[0]?.results.map((row) => row.id)
    ).toEqual([DRAFT_2]);
  });

  it("promotes a draft, drops the row and toasts", async () => {
    const t = transport(() => ({
      status: 200,
      body: {
        id: DRAFT_1,
        name: "One",
        project_id: PROJECT,
        sequence_id: 5,
        state_id: null,
        priority: "none",
        sort_order: 1,
        parent_id: null,
        description_html: "",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        deleted_at: null,
      },
    }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([draft(DRAFT_1, "One")])]));
    const { result } = renderHook(() => usePromoteDraft(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ draftId: DRAFT_1, input: { name: "One", project_id: PROJECT } });
    });
    expect(t.calls.find((call) => call.method === "POST")).toMatchObject({
      url: `https://api.test/api/workspaces/acme/draft-to-issue/${DRAFT_1}/`,
    });
    expect(toasts.notifySuccess).toHaveBeenCalledWith(DRAFT_PROMOTED_TITLE, DRAFT_PROMOTED_BODY);
    expect(
      queryClient.getQueryData<{ pages: DraftIssueListResponse[] }>(draftKeys.list(WS))?.pages[0]?.results
    ).toEqual([]);
  });

  it("leaves a failed promote for the caller to explain and keeps the row", async () => {
    const t = transport(() => ({ status: 500, body: { error: "boom" } }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([draft(DRAFT_1, "One")])]));
    const { result } = renderHook(() => usePromoteDraft(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current
        .mutateAsync({ draftId: DRAFT_1, input: { name: "One", project_id: PROJECT } })
        .catch(() => undefined);
    });
    expect(toasts.notifyError).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        queryClient.getQueryData<{ pages: DraftIssueListResponse[] }>(draftKeys.list(WS))?.pages[0]?.results
      ).toHaveLength(1)
    );
  });

  it("creates a draft and re-reads the list", async () => {
    const posted = draft(DRAFT_2, "New");
    const t = transport((_url, method) =>
      method === "POST" ? { status: 201, body: posted } : { status: 200, body: page([posted]) }
    );
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    queryClient.setQueryData(draftKeys.list(WS), infinite([page([])]));
    const { result } = renderHook(() => useCreateDraft(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ name: "New", project_id: PROJECT });
    });
    expect(t.calls.find((call) => call.method === "POST")).toMatchObject({
      url: "https://api.test/api/workspaces/acme/draft-issues/",
    });
    expect(toasts.notifySuccess).toHaveBeenCalledWith(DRAFT_CREATED_TITLE, DRAFT_CREATED_BODY);
    await waitFor(() => expect(t.calls.filter((call) => call.method === "GET").length).toBeGreaterThan(0), {
      timeout: 3000,
    });
  });

  it("explains a failed create", async () => {
    const t = transport(() => ({ status: 500, body: { error: "boom" } }));
    createAppClient("https://api.test", t.run as never);
    const queryClient = newClient();
    const { result } = renderHook(() => useCreateDraft(WS), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ name: "New" }).catch(() => undefined);
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(DRAFT_CREATE_FAILED_TITLE, DRAFT_CREATE_FAILED_BODY);
  });
});
