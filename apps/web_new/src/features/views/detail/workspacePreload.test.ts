// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { ApiError, createClient, type GlobalIssueListResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { issueKeys } from "../../issues/api/keys.js";
import { makeView } from "../testing.js";
import { preloadWorkspaceView } from "./workspacePreload.js";

const PARAMS = { ws: "acme", viewId: "223e4567-e89b-12d3-a456-426614174001" };

function emptyGlobalIssues(): GlobalIssueListResponse {
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
    project: null,
    name: "Triage",
    display_filters: { layout: "spreadsheet", order_by: "-updated_at" },
    display_properties: {},
    rich_filters: { priority: "urgent" },
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

describe("preloadWorkspaceView", () => {
  it("reads the custom record then the scoped first page", async () => {
    const { client, calls } = clientFor((url) => {
      if (url.includes(`/views/${PARAMS.viewId}/`)) {
        return { status: 200, body: viewRecord() };
      }
      if (url.includes("/views/")) return { status: 200, body: [viewRecord()] };
      return { status: 200, body: emptyGlobalIssues() };
    });
    const queryClient = newClient();
    await preloadWorkspaceView(queryClient, client, PARAMS);

    expect(calls[0]).toContain("/views/?");
    expect(calls[1]).toContain(`/views/${PARAMS.viewId}/`);
    expect(calls[2]).toContain("/api/workspaces/acme/issues/?");
    expect(calls[2]).toContain("order_by=-updated_at");
    expect(decodeURIComponent(calls[2] ?? "")).toContain('"priority":"urgent"');
  });

  it("reads all-issues plain and skips the member-scoped defaults", async () => {
    const { client, calls } = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: [] } : { status: 200, body: emptyGlobalIssues() }
    );
    const queryClient = newClient();
    await preloadWorkspaceView(queryClient, client, { ws: "acme", viewId: "all-issues" });
    expect(calls).toHaveLength(2);
    expect(calls[1]).toContain("/api/workspaces/acme/issues/?");
    expect(calls[1]).not.toContain("order_by=");
    expect(calls[1]).not.toContain("filters=");

    const scoped = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: [] } : { status: 200, body: emptyGlobalIssues() }
    );
    await preloadWorkspaceView(newClient(), scoped.client, { ws: "acme", viewId: "assigned" });
    expect(scoped.calls).toHaveLength(1);
  });

  it("settles reads on cache entries a second visit reuses", async () => {
    const { client, calls } = clientFor((url) => {
      if (url.includes(`/views/${PARAMS.viewId}/`)) {
        return { status: 200, body: viewRecord() };
      }
      if (url.includes("/views/")) return { status: 200, body: [viewRecord()] };
      return { status: 200, body: emptyGlobalIssues() };
    });
    const queryClient = newClient();
    await preloadWorkspaceView(queryClient, client, PARAMS);
    const first = calls.length;
    await preloadWorkspaceView(queryClient, client, PARAMS);
    expect(calls.length).toBe(first);
  });

  it("never retrieves an id the visible list hides", async () => {
    const { client, calls } = clientFor((url) =>
      url.includes("/views/") ? { status: 200, body: [] } : { status: 200, body: emptyGlobalIssues() }
    );
    const queryClient = newClient();
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
    expect(calls).toHaveLength(1);
  });

  it("keeps later reads off when the customs read fails", async () => {
    const { client, calls } = clientFor(() => ({ status: 500, body: {} }));
    const queryClient = newClient();
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
    expect(calls).toHaveLength(1);
    expect(queryClient.getQueryCache().findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "global"] })).toHaveLength(0);
  });

  it("keeps the list read off when the record read fails", async () => {
    const { client, calls } = clientFor((url) =>
      url.includes(`/views/${PARAMS.viewId}/`) ? { status: 404, body: {} } : { status: 200, body: [viewRecord()] }
    );
    const queryClient = newClient();
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
    expect(calls).toHaveLength(2);
    expect(queryClient.getQueryCache().findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "global"] })).toHaveLength(0);
  });

  it("treats a blank record as missing and never reads the list", async () => {
    const { client, calls } = clientFor((url) => {
      if (url.includes(`/views/${PARAMS.viewId}/`)) {
        return { status: 200, body: { ...viewRecord(), id: "", name: "" } };
      }
      if (url.includes("/views/")) return { status: 200, body: [viewRecord()] };
      return { status: 200, body: emptyGlobalIssues() };
    });
    const queryClient = newClient();
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
    expect(calls).toHaveLength(2);
  });

  it("does not reject when the issue read fails", async () => {
    const { client } = clientFor((url) => {
      if (url.includes("/views/")) return { status: 200, body: [viewRecord()] };
      return { status: 500, body: {} };
    });
    const queryClient = newClient();
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
  });

  it("treats a transport failure as the error state", async () => {
    const queryClient = newClient();
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async () => {
        throw new ApiError({ message: "boom", status: 500, code: "server", body: null });
      },
    });
    await expect(preloadWorkspaceView(queryClient, client, PARAMS)).resolves.toBeUndefined();
  });
});
