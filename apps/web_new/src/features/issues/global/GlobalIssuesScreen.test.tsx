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
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/api/keys.js";
import { GLOBAL_LOAD_MORE_LABEL, GlobalIssuesScreen, globalIssueKey } from "./GlobalIssuesScreen.js";
import { globalIssuesQueryOptions } from "./queries.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "323e4567-e89b-12d3-a456-426614174002";

function row(seed: string, overrides: Partial<GlobalIssueRow> = {}): GlobalIssueRow {
  const id = idFor(seed);
  return {
    id,
    name: `Issue ${seed}`,
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

/** A stable UUID for a short test seed, so the row schema accepts it. */
function idFor(seed: string): string {
  let hex = "";
  for (const char of seed) hex += char.charCodeAt(0).toString(16).padStart(2, "0");
  const filled = `${hex}${"0".repeat(40)}`.slice(0, 32);
  return `${filled.slice(0, 8)}-${filled.slice(8, 12)}-4${filled.slice(13, 16)}-8${filled.slice(17, 20)}-${filled.slice(20, 32)}`;
}

function projectRow(id: string, identifier: string): Record<string, unknown> {
  return { id, name: identifier, identifier, member_role: 15, archived_at: null };
}

/** One response page, with the cursor fields the read expects. */
function page(results: GlobalIssueRow[], options: { total?: number; nextCursor?: string } = {}) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: options.total ?? results.length,
    next_cursor: options.nextCursor ?? "",
    prev_cursor: "",
    next_page_results: options.nextCursor !== undefined && options.nextCursor !== "",
    prev_page_results: false,
    count: results.length,
    total_results: options.total ?? results.length,
    results,
  };
}

function renderScreen(
  issues: { results: GlobalIssueRow[]; total_count?: number },
  routeFilters: Record<string, string> = {},
  options: {
    withoutWorkspaceParam?: boolean;
    /** The continuation the transport answers for a cursor request. */
    nextPage?: ReturnType<typeof page>;
    /** Records each read URL, for proving the cursor round-trip. */
    onFetch?: (url: string) => void;
  } = {}
) {
  createAppClient("https://api.test", ((url: string) => {
    options.onFetch?.(url);
    const body = url.includes("cursor=") ? (networkPages?.next ?? null) : (networkPages?.first ?? null);
    if (body === undefined || body === null) return Promise.reject(new Error("unexpected network"));
    return Promise.resolve({
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    });
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  const firstPage = page(issues.results, {
    ...(issues.total_count === undefined ? {} : { total: issues.total_count }),
    ...(options.nextPage === undefined ? {} : { nextCursor: "page-2" }),
  });
  networkPages = { first: firstPage, ...(options.nextPage === undefined ? {} : { next: options.nextPage }) };
  queryClient.setQueryData(
    globalIssuesQueryOptions(getAppClient(), options.withoutWorkspaceParam ? "" : SLUG).queryKey,
    { pages: [firstPage], pageParams: [null] }
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

/** Every sentinel observer mounted this test, so a test can scroll it. */
const capturedObservers: Array<{ trigger(): void; disconnect(): void }> = [];

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region") return 400;
      if (this.hasAttribute("data-index")) return 40;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
  capturedObservers.length = 0;
  class TestIntersectionObserver {
    private readonly callback: IntersectionObserverCallback;
    constructor(callback: IntersectionObserverCallback) {
      this.callback = callback;
      capturedObservers.push(this);
    }
    observe(): void {}
    disconnect(): void {}
    /** Drive the sentinel as the browser would when it scrolls into view. */
    trigger(): void {
      this.callback([{ isIntersecting: true } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
  }
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
});

// The infinite-query mount refetches its first window; answer with the
// seeded first page for the plain read and the seeded second page for a
// cursor continuation.
let networkPages: { first: unknown; next?: unknown } | null = null;
afterEach(() => {
  networkPages = null;
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

  it("appends the next cursor page instead of stopping at the first window (ISS-223)", async () => {
    const first = Array.from({ length: 100 }, (_unused, index) => row(`p1-${index}`, { sequence_id: index + 1 }));
    const second = Array.from({ length: 20 }, (_unused, index) => row(`p2-${index}`, { sequence_id: 101 + index }));
    const seen: string[] = [];
    const { queryClient } = renderScreen(
      { results: first, total_count: 120 },
      {},
      { nextPage: page(second, { total: 120 }), onFetch: (url) => seen.push(url) }
    );
    // The header reports the server total while the first window mounts.
    expect(await screen.findByText("120")).toBeInTheDocument();
    const more = await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await userEvent.setup().click(more);
    // The virtual list mounts only the visible window, so the appended
    // page is proven through the cached rows and the retired control.
    await waitFor(() =>
      expect(
        queryClient.getQueryData<{ pages: unknown[] }>(globalIssuesQueryOptions(getAppClient(), SLUG).queryKey)
      ).toHaveProperty("pages.1.results.19.id", idFor("p2-19"))
    );
    await waitFor(() => expect(screen.queryByRole("button", { name: GLOBAL_LOAD_MORE_LABEL })).not.toBeInTheDocument());
  });

  it("sends the cursor to the server when the reader asks for more", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 2 },
      {},
      {
        nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 2 }),
        onFetch: (url) => seen.push(url),
      }
    );
    expect((await screen.findAllByText("Issue p1-0")).length).toBeGreaterThan(0);
    const more = await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await userEvent.setup().click(more);
    expect(seen.some((url) => url.includes("cursor=page-2"))).toBe(true);
  });

  it("asks for the next page when the sentinel scrolls into view", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 2 },
      {},
      { nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 2 }), onFetch: (url) => seen.push(url) }
    );
    await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await waitFor(() => expect(capturedObservers.length).toBeGreaterThan(0));
    act(() => capturedObservers.at(-1)?.trigger());
    await waitFor(() => expect(seen.some((url) => url.includes("cursor=page-2"))).toBe(true));
  });

  it("asks for one continuation at a time while a page is in flight", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 3 },
      {},
      {
        nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 3, nextCursor: "page-3" }),
        onFetch: (url) => seen.push(url),
      }
    );
    await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await waitFor(() => expect(capturedObservers.length).toBeGreaterThan(0));
    const observer = capturedObservers.at(-1);
    act(() => observer?.trigger());
    act(() => observer?.trigger());
    await waitFor(() => expect(seen.filter((url) => url.includes("cursor=page-2")).length).toBe(1));
  });

  it("does not read again once the chain ended", async () => {
    renderScreen({ results: [row("only", { sequence_id: 1 })] });
    await screen.findByText("Issue only");
    act(() => capturedObservers.forEach((observer) => observer.trigger()));
    // With no continuation there is no footer sentinel at all, so the
    // observer list stays empty and nothing fires.
    expect(capturedObservers).toHaveLength(0);
  });
});
