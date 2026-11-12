// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { ApiError, createClient, type IssueListResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { issueKeys } from "../../issues/api/keys.js";
import { makeView } from "../testing.js";
import { preloadSavedView } from "./preload.js";

const PARAMS = {
  ws: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  viewId: "323e4567-e89b-12d3-a456-426614174002",
};

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

function viewRecord(overrides: Record<string, unknown> = {}) {
  return makeView({
    id: PARAMS.viewId,
    project: PARAMS.projectId,
    name: "Unstarted",
    display_filters: { layout: "list", order_by: "-updated_at" },
    rich_filters: { and: [{ state_group__in: "unstarted" }] },
    ...overrides,
  });
}

function clientFor(handler: (url: string) => { status: number; body: unknown }) {
  const calls: string[] = [];
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      calls.push(url);
      const { status, body } = handler(url);
      return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
    },
  });
  return { client, calls };
}

function newClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

describe("preloadSavedView", () => {
  it("reads the view record then the list in its stored order and expression", async () => {
    const { client, calls } = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: viewRecord() } : { status: 200, body: emptyIssues() }
    );
    const queryClient = newClient();
    await preloadSavedView(queryClient, client, PARAMS, {});

    expect(calls[0]).toContain(`/views/${PARAMS.viewId}/`);
    expect(calls[1]).toContain("/issues/?");
    expect(calls[1]).toContain("order_by=-updated_at");
    const listKeys = queryClient
      .getQueryCache()
      .findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "list"] })
      .map((query) => JSON.stringify(query.queryKey));
    expect(listKeys.some((key) => key.includes("state_group__in"))).toBe(true);
  });

  it("settles both reads on cache entries a second visit reuses", async () => {
    const { client, calls } = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: viewRecord() } : { status: 200, body: emptyIssues() }
    );
    const queryClient = newClient();
    await preloadSavedView(queryClient, client, PARAMS, {});
    const first = calls.length;
    await preloadSavedView(queryClient, client, PARAMS, {});
    expect(calls.length).toBe(first);
  });

  it("keeps the list read off when the view read fails", async () => {
    const { client, calls } = clientFor(() => ({ status: 404, body: {} }));
    const queryClient = newClient();
    // A failed view read is the missing-view state, not a load error.
    await expect(preloadSavedView(queryClient, client, PARAMS, {})).resolves.toBeUndefined();
    expect(calls).toHaveLength(1);
    expect(queryClient.getQueryCache().findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "list"] })).toHaveLength(0);
  });

  it("does not reject when the issue list read fails", async () => {
    const { client } = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: viewRecord() } : { status: 500, body: {} }
    );
    const queryClient = newClient();
    await expect(preloadSavedView(queryClient, client, PARAMS, {})).resolves.toBeUndefined();
  });

  it("treats a blank record as missing and never reads the list", async () => {
    const { client, calls } = clientFor(() => ({
      status: 200,
      body: { ...viewRecord(), id: "", name: "" },
    }));
    const queryClient = newClient();
    await expect(preloadSavedView(queryClient, client, PARAMS, {})).resolves.toBeUndefined();
    expect(calls).toHaveLength(1);
  });

  it("treats a forbidden read as the missing-view state", async () => {
    const { client } = clientFor(() => ({ status: 403, body: {} }));
    const queryClient = newClient();
    await expect(preloadSavedView(queryClient, client, PARAMS, {})).resolves.toBeUndefined();
  });

  it("treats a transport failure as the missing-view state", async () => {
    const queryClient = newClient();
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async () => {
        throw new ApiError({ message: "boom", status: 500, code: "server", body: null });
      },
    });
    await expect(preloadSavedView(queryClient, client, PARAMS, {})).resolves.toBeUndefined();
  });
});
