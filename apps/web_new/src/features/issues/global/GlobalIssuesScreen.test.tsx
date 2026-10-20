// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { GlobalIssueRow } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/api/keys.js";
import { GlobalIssuesScreen, globalIssueKey } from "./GlobalIssuesScreen.js";
import { globalIssuesQueryOptions } from "./queries.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "323e4567-e89b-12d3-a456-426614174002";

function row(id: string, overrides: Partial<GlobalIssueRow> = {}): GlobalIssueRow {
  return {
    id,
    name: `Issue ${id.slice(-2)}`,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state__group: null,
    ...overrides,
  };
}

function projectRow(id: string, identifier: string): Record<string, unknown> {
  return { id, name: identifier, identifier, member_role: 15, archived_at: null };
}

function renderScreen(
  issues: { results: GlobalIssueRow[]; total_count?: number },
  routeFilters: Record<string, string> = {},
  options: { withoutWorkspaceParam?: boolean } = {}
) {
  createAppClient("https://api.test", (() => Promise.reject(new Error("unexpected network"))) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  queryClient.setQueryData(
    globalIssuesQueryOptions(getAppClient(), options.withoutWorkspaceParam ? "" : SLUG).queryKey,
    {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: issues.total_count ?? issues.results.length,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: issues.results.length,
      total_results: issues.results.length,
      results: issues.results,
    }
  );
  queryClient.setQueryData(projectKeys.list(options.withoutWorkspaceParam ? "" : SLUG), [
    projectRow(PROJECT, "WEB"),
    projectRow(OTHER_PROJECT, "API"),
  ]);
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const layout = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const browse = createRoute({
    getParentRoute: () => layout,
    path: "/browse/$issueKey",
    component: () => <div data-testid="browse" />,
  });
  const allIssuesParent = options.withoutWorkspaceParam ? rootRoute : layout;
  const allIssuesPath = options.withoutWorkspaceParam ? "/workspace-views/all-issues" : "/workspace-views/all-issues";
  const allIssues = createRoute({
    getParentRoute: () => allIssuesParent,
    path: allIssuesPath,
    validateSearch: (search: Record<string, unknown>) => search as Record<string, string>,
    component: () => <GlobalIssuesScreen />,
  });
  const query = new URLSearchParams(routeFilters).toString();
  const router = createRouter({
    routeTree: options.withoutWorkspaceParam
      ? rootRoute.addChildren([layout.addChildren([browse]), allIssues])
      : rootRoute.addChildren([layout.addChildren([browse, allIssues])]),
    context: { queryClient },
    history: createMemoryHistory({
      initialEntries: [
        options.withoutWorkspaceParam
          ? `/workspace-views/all-issues${query ? `?${query}` : ""}`
          : `/${SLUG}/workspace-views/all-issues${query ? `?${query}` : ""}`,
      ],
    }),
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { view, queryClient, router };
}

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

// jsdom reports zero sizes, so the virtual list would compute an empty
// window. Give the scroll region and rows a height so rows mount.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region") return 400;
      if (this.hasAttribute("data-index")) return 40;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
  class TestIntersectionObserver {
    observe(): void {}
    disconnect(): void {}
  }
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
});

describe("globalIssueKey", () => {
  it("prefixes the sequence with the project identifier", () => {
    expect(globalIssueKey("WEB", { sequence_id: 42 })).toBe("WEB-42");
    expect(globalIssueKey(undefined, { sequence_id: 7 })).toBe("?-7");
  });
});

describe("GlobalIssuesScreen", () => {
  it("lists rows from every project with their short keys", async () => {
    renderScreen({ results: [row("a1"), row("b2", { project_id: OTHER_PROJECT, sequence_id: 9 })] });
    expect(await screen.findByText("Issue a1")).toBeInTheDocument();
    expect(screen.getByText("Issue b2")).toBeInTheDocument();
    expect(screen.getByText("WEB-1")).toBeInTheDocument();
    expect(screen.getByText("API-9")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("renders the same unfiltered rows when URL filters are present (ISS-223)", async () => {
    renderScreen({ results: [row("a1"), row("b2")] }, { priority: "urgent", order_by: "-created_at" });
    expect(await screen.findByText("Issue a1")).toBeInTheDocument();
    expect(screen.getByText("Issue b2")).toBeInTheDocument();
  });

  it("offers a filter-aware empty state and the plain one", async () => {
    const filtered = renderScreen({ results: [] }, { priority: "urgent" });
    expect(await screen.findByText("No issues match these filters.")).toBeInTheDocument();
    filtered.view.unmount();

    renderScreen({ results: [] });
    expect(await screen.findByText("Issues from every project will appear here.")).toBeInTheDocument();
  });

  it("falls back to an empty slug when the route carries no workspace param", async () => {
    renderScreen({ results: [row("a1", { sequence_id: 3 })] }, {}, { withoutWorkspaceParam: true });
    const link = await screen.findByRole("link", { name: /WEB-3/ });
    expect(link.getAttribute("href")).toBe("/browse/WEB-3");
  });

  it("links a row to the workspace browse route", async () => {
    const { queryClient } = renderScreen({ results: [row("a1", { sequence_id: 5 })] });
    const link = await screen.findByRole("link", { name: /WEB-5/ });
    expect(link.getAttribute("href")).toBe(`/${SLUG}/browse/WEB-5`);
    queryClient.clear();
  });
});
