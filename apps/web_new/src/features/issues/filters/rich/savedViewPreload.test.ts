// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view preload (ISS-104, NEWFRONT-677): the route module is eager, so
// the view record and list reads, and the list leaves they await, live in
// this module with the stored-expression imports. The cases prove the read
// order, that the view's own stored order and expression reach the list
// query, and that a gate denial propagates to the caller unchanged.
import { QueryClient } from "@tanstack/react-query";
import { createClient, type IssueListResponse } from "@pidash/api-client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { prefetchProjectView } from "./savedViewPreload.js";

const PARAMS = {
  ws: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  viewId: "323e4567-e89b-12d3-a456-426614174002",
};
const USER = "123e4567-e89b-12d3-a456-426614174000";
const EXPRESSION = { and: [{ state_group__in: "unstarted" }] };

const leaves = vi.hoisted(() => ({ primed: 0, loaded: 0 }));

vi.mock("../../components/lazyLeaves.js", () => ({
  primeIssueListLeaves: () => {
    leaves.primed += 1;
  },
  loadIssueListLeaves: () => {
    leaves.loaded += 1;
    return Promise.resolve();
  },
}));

function viewRecord(overrides: Record<string, unknown> = {}) {
  return {
    id: PARAMS.viewId,
    workspace: USER,
    project: PARAMS.projectId,
    name: "Unstarted work",
    description: "",
    query: {},
    filters: {},
    display_filters: { layout: "list", order_by: "-updated_at" },
    display_properties: {},
    rich_filters: EXPRESSION,
    access: 0,
    sort_order: 1,
    logo_props: {},
    owned_by: USER,
    is_locked: false,
    archived_at: null,
    is_favorite: false,
    created_by: USER,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function issueResponse(): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: [],
  };
}

interface Reply {
  status?: number;
  body: unknown;
}

/** A client that answers each request by URL and records the order. */
function recordingClient(reply: (url: string) => Reply | Promise<Reply>) {
  const urls: string[] = [];
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      urls.push(url);
      const answer = await reply(url);
      return {
        status: answer.status ?? 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(answer.body),
      };
    },
  });
  return { client, urls };
}

let queryClient: QueryClient;

beforeEach(() => {
  leaves.primed = 0;
  leaves.loaded = 0;
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
});

describe("saved-view preload", () => {
  it("primes the list leaves, reads the view first and the list in the view's stored order", async () => {
    const { client, urls } = recordingClient((url) =>
      url.includes("/views/") ? { body: viewRecord() } : { body: issueResponse() }
    );
    await prefetchProjectView(queryClient, client, PARAMS, {});
    expect(leaves.primed).toBe(1);
    expect(leaves.loaded).toBe(1);
    expect(urls[0]).toContain(`/views/${PARAMS.viewId}/`);
    const listUrl = urls.find((url) => url.includes("/issues/?"));
    expect(listUrl).toContain("order_by=-updated_at");
    expect(decodeURIComponent(listUrl ?? "")).toContain("state_group__in");
  });

  it("keys the list on the issues screen's validated search as given", async () => {
    const { client } = recordingClient((url) =>
      url.includes("/views/") ? { body: viewRecord() } : { body: issueResponse() }
    );
    await prefetchProjectView(queryClient, client, PARAMS, { layout: "compact" });
    const [recordKey, listKey] = queryClient
      .getQueryCache()
      .getAll()
      .map((query) => query.queryKey);
    expect(recordKey).toEqual(["ws", "acme", "issues", "saved-view", PARAMS.projectId, PARAMS.viewId]);
    // The list key carries the search object and the view's stored order.
    expect(listKey).toEqual([
      "ws",
      "acme",
      "issues",
      "list",
      PARAMS.projectId,
      { layout: "compact" },
      "-updated_at",
      '{"state_group__in":"unstarted"}',
      null,
    ]);
  });

  it("re-reads nothing when the record and the list are already cached", async () => {
    let calls = 0;
    const { client } = recordingClient((url) => {
      calls += 1;
      return url.includes("/views/") ? { body: viewRecord() } : { body: issueResponse() };
    });
    await prefetchProjectView(queryClient, client, PARAMS, {});
    const first = calls;
    await prefetchProjectView(queryClient, client, PARAMS, {});
    expect(calls).toBe(first);
    // The leaves still settle for the second visit.
    expect(leaves.loaded).toBe(2);
  });

  it.each([403, 404, 409, 500])("lets a %s rejection reach the route unchanged", async (status) => {
    const { client } = recordingClient(() => ({ status, body: {} }));
    await expect(prefetchProjectView(queryClient, client, PARAMS, {})).rejects.toMatchObject({ status });
    // The list was never asked for: the view read gates it.
    expect(leaves.loaded).toBe(0);
  });
});
