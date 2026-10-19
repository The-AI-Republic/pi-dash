// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view route preload contract (NEWFRONT-677). The route file is part
// of the router's eager tree, so the screen and the view's stored-expression
// reads ride a chunk the route requests on visit. These cases pin what
// `beforeLoad` guarantees: one visit reads the view record, then the list in
// the view's stored order and expression, and any failure settles (the gate
// denials pass through to the project gate, everything else throws to the
// route's error state) without leaving the request unsettled. The screen's
// own render path is covered by index.test.tsx, which mounts the route
// component against the same lazy module.
import { QueryClient } from "@tanstack/react-query";
import { ApiError, createClient, type IssueListResponse } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { issueKeys } from "../../../../../../../features/issues/api/keys.js";
import { Route } from "./index.js";

const PARAMS = {
  ws: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  viewId: "323e4567-e89b-12d3-a456-426614174002",
};
const USER = "123e4567-e89b-12d3-a456-426614174000";

function viewRecord() {
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
    rich_filters: { and: [{ state_group__in: "unstarted" }] },
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
  };
}

function emptyIssues(): IssueListResponse {
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

function clientFor(status: number, body: unknown) {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({ status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) }),
  });
}

function contextFor(status = 200, body: unknown = viewRecord()) {
  return {
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
    apiClient: clientFor(status, body),
  };
}

function runBeforeLoad(context: ReturnType<typeof contextFor>, apiClient?: unknown) {
  const input = { context: apiClient === undefined ? context : { ...context, apiClient }, params: PARAMS, search: {} };
  return Route.options.beforeLoad?.(input as never);
}

describe("saved-view route preload", () => {
  it("reads the view record, then the list in its stored order and expression", async () => {
    const context = contextFor();
    const urls: string[] = [];
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        urls.push(url);
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify(url.includes("/views/") ? viewRecord() : emptyIssues()),
        };
      },
    });
    await runBeforeLoad(context, apiClient);
    expect(urls[0]).toContain(`/views/${PARAMS.viewId}/`);
    expect(urls[1]).toContain("/issues/?");
    expect(urls[1]).toContain("order_by=-updated_at");
    // The list query carries the view's stored expression.
    const listKey = context.queryClient
      .getQueryCache()
      .findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "list"] })
      .map((query) => JSON.stringify(query.queryKey));
    expect(listKey.some((key) => key.includes("state_group__in"))).toBe(true);
  });

  it("settles both visits on the same cache entries", async () => {
    const context = contextFor();
    const spy = vi.fn(async (url: string) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(url.includes("/views/") ? viewRecord() : emptyIssues()),
    }));
    const apiClient = createClient({ baseUrl: "https://api.test", transport: spy });
    await runBeforeLoad(context, apiClient);
    const calls = spy.mock.calls.length;
    await runBeforeLoad(context, apiClient);
    expect(spy.mock.calls.length).toBe(calls);
  });

  it.each([403, 404, 409])("passes a %s denial through to the project gate", async (status) => {
    await expect(runBeforeLoad(contextFor(status, {}))).resolves.toBeUndefined();
  });

  it("throws any other failure to the route's load error", async () => {
    await expect(runBeforeLoad(contextFor(500, {}))).rejects.toBeInstanceOf(ApiError);
  });
});
