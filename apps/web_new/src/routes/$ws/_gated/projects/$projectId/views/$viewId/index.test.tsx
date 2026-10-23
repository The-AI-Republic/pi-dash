// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view detail route (parity/specs/views.md §7). The route file is in
// the router's eager tree, so it must stay thin: these cases pin that the
// visit starts and awaits the same detail chunk once, that the view's own
// reads are prefetched before the screen commits, and that a missing view
// is not turned into a route-level load error.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createClient, type IssueListResponse } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { issueKeys } from "../../../../../../../features/issues/api/keys.js";
import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { makeView } from "../../../../../../../features/views/testing.js";
import { Route } from "./index.js";
import { loadSavedViewDetail } from "../../../../../../../features/views/index.js";

const PARAMS = {
  ws: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  viewId: "323e4567-e89b-12d3-a456-426614174002",
};

function viewRecord(overrides: Record<string, unknown> = {}) {
  return makeView({
    id: PARAMS.viewId,
    project: PARAMS.projectId,
    name: "Unstarted work",
    display_filters: { layout: "list", order_by: "-updated_at" },
    rich_filters: { and: [{ state_group__in: "unstarted" }] },
    ...overrides,
  });
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

function projectRow() {
  return {
    id: PARAMS.projectId,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function contextFor(handler: (url: string) => { status: number; body: unknown } = () => ({ status: 200, body: [] })) {
  const calls: string[] = [];
  const apiClient = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      calls.push(url);
      const { status, body } = handler(url);
      return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
    },
  });
  return {
    calls,
    context: {
      queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
      apiClient,
    },
  };
}

function runBeforeLoad(context: ReturnType<typeof contextFor>["context"]) {
  return Route.options.beforeLoad?.({ context, params: PARAMS, search: {} } as never);
}

const happyPath = (url: string) => {
  if (url.endsWith("/projects/")) return { status: 200, body: [projectRow()] };
  return url.includes("/views/") ? { status: 200, body: viewRecord() } : { status: 200, body: emptyIssues() };
};

describe("saved-view detail route preload", () => {
  it("shows the list skeleton while the route loads", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("missing pending component");
    render(<Pending />);
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("renders the resolved screen from the route params and search", async () => {
    await loadSavedViewDetail();
    const paramsSpy = vi.spyOn(Route, "useParams").mockReturnValue(PARAMS as never);
    const searchSpy = vi.spyOn(Route, "useSearch").mockReturnValue({ layout: "compact" } as never);
    const { context } = contextFor(happyPath);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/api/users/me/profile/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ start_of_the_week: 1 }),
        };
      }
      const { status, body } = happyPath(url);
      return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
    }) as never);
    render(
      <QueryClientProvider client={context.queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("region", { name: "Saved view" })).toBeInTheDocument();
    expect(paramsSpy).toHaveBeenCalled();
    expect(searchSpy).toHaveBeenCalled();
    paramsSpy.mockRestore();
    searchSpy.mockRestore();
    resetAppClient();
  });

  it("reads the view record, then the list in its stored order and expression", async () => {
    const { context, calls } = contextFor(happyPath);
    await runBeforeLoad(context);
    expect(calls[0]).toContain(`/views/${PARAMS.viewId}/`);
    expect(calls[1]).toContain("/issues/?");
    expect(calls[1]).toContain("order_by=-updated_at");
    const listKeys = context.queryClient
      .getQueryCache()
      .findAll({ queryKey: [...issueKeys.all(PARAMS.ws), "list"] })
      .map((query) => JSON.stringify(query.queryKey));
    expect(listKeys.some((key) => key.includes("state_group__in"))).toBe(true);
  });

  it("reuses the settled cache on a second visit", async () => {
    const { context, calls } = contextFor(happyPath);
    await runBeforeLoad(context);
    const settled = calls.length;
    await runBeforeLoad(context);
    expect(calls.length).toBe(settled);
  });

  it.each([403, 404, 500])("settles a %s view read into the missing-view state", async (status) => {
    const { context } = contextFor(() => ({ status, body: {} }));
    await expect(runBeforeLoad(context)).resolves.toBeUndefined();
  });

  it("settles a blank record into the missing-view state without reading the list", async () => {
    const { context, calls } = contextFor(() => ({ status: 200, body: { ...viewRecord(), id: "", name: "" } }));
    await expect(runBeforeLoad(context)).resolves.toBeUndefined();
    expect(calls).toHaveLength(1);
  });

  it("swallows a project gate denial rather than failing navigation", async () => {
    const { context } = contextFor(() => ({ status: 403, body: {} }));
    await expect(runBeforeLoad(context)).resolves.toBeUndefined();
  });

  it("re-raises a non-gate chunk failure so the route reports it", async () => {
    vi.resetModules();
    vi.doMock("../../../../../../../features/views/detail/lazy.ts", () => ({
      loadSavedViewDetail: () => Promise.reject(new Error("chunk failed")),
      resolvedSavedViewDetail: () => ({ SavedViewScreen: () => null, preloadSavedView: async () => undefined }),
    }));
    const { Route: FreshRoute } = await import("./index.js");
    await expect(FreshRoute.options.beforeLoad?.({ context: {}, params: PARAMS, search: {} } as never)).rejects.toThrow(
      "chunk failed"
    );
    vi.doUnmock("../../../../../../../features/views/detail/lazy.ts");
    vi.resetModules();
  });

  it("reads the project row so the trail and tab title resolve with the screen", async () => {
    const { context } = contextFor(happyPath);
    // `loader` owns the project read; `beforeLoad` owns the view reads.
    const loader = Route.options.loader as unknown as (input: unknown) => Promise<unknown>;
    await loader({ context, params: PARAMS, deps: {} });
    expect(context.queryClient.getQueryData(["ws", PARAMS.ws, "projects", "list"])).toBeDefined();
  });
});
