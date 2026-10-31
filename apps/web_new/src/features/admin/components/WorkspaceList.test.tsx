// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute as createTestRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  WORKSPACE_CREATE_PATH,
  WORKSPACE_LIST_SKELETON_ROWS,
  WORKSPACE_SLUG_TOOLTIP,
  WorkspaceList,
} from "./WorkspaceList.js";

let workspaceId = 0;

function workspaceRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  workspaceId += 1;
  return {
    id: `aaaaaaaa-aaaa-4aaa-8aaa-${String(workspaceId).padStart(12, "0")}`,
    name: `Workspace ${workspaceId}`,
    slug: `workspace-${workspaceId}`,
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      email: "owner@example.com",
      first_name: "Ow",
      last_name: "Ner",
    },
    organization_size: "2-10",
    timezone: "UTC",
    background_color: "#ffffff",
    total_projects: 2,
    total_members: 3,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function workspacePage(results: Record<string, unknown>[], nextCursor: string): Record<string, unknown> {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: nextCursor,
    prev_cursor: "",
    next_page_results: nextCursor !== "",
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
  };
}

function jsonResponse(url: string, status: number, body: unknown) {
  return Promise.resolve({
    status,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve(JSON.stringify(body)),
  });
}

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function renderList(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const pageRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/workspace",
    component: WorkspaceList,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([pageRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

function workspaceRows(): HTMLElement[] {
  return screen.getAllByTestId("admin-workspace-row");
}

afterEach(() => {
  resetAppClient();
});

describe("WorkspaceList", () => {
  it("pins the section constants", () => {
    expect(WORKSPACE_CREATE_PATH).toBe("/god-mode/workspace/create/");
    expect(WORKSPACE_LIST_SKELETON_ROWS).toBe(4);
    expect(WORKSPACE_SLUG_TOOLTIP).toBe("The short name used in this workspace's web address.");
  });

  it("shows a four-row skeleton with no count on first load", async () => {
    createAppClient("https://api.test", hangingTransport());
    renderList();
    const skeleton = await screen.findByTestId("admin-workspace-list-skeleton");
    expect(skeleton).toHaveAttribute("role", "status");
    expect(skeleton.childElementCount).toBe(4);
    expect(screen.queryByTestId("admin-workspace-count")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-helper")).toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-create")).toHaveAttribute("href", "/god-mode/workspace/create/");
  });

  it("renders the count with per-row detail matching the server", async () => {
    const rows = [
      workspaceRow({ name: "Acme", slug: "acme", logo_url: "https://cdn.test/acme.png" }),
      workspaceRow({ name: "Globex", slug: "globex", owner: null, total_projects: null, total_members: null }),
    ];
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, workspacePage(rows, ""))) as Transport);
    renderList();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("2"));
    expect(screen.queryByTestId("admin-workspace-count-spinner")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();

    const rendered = workspaceRows();
    expect(rendered).toHaveLength(2);
    const acme = within(rendered[0]!);
    expect(acme.getByTestId("admin-workspace-row-name")).toHaveTextContent("Acme");
    expect(acme.getByTestId("admin-workspace-row-slug")).toHaveTextContent("acme");
    expect(acme.getByTestId("admin-workspace-row-logo")).toHaveAttribute("src", "https://cdn.test/acme.png");
    expect(acme.queryByTestId("admin-workspace-row-letter")).not.toBeInTheDocument();
    expect(acme.getByTestId("admin-workspace-row-owner")).toHaveTextContent("Owner owner@example.com");
    expect(acme.getByTestId("admin-workspace-row-counts")).toHaveTextContent("2 projects · 3 members");

    const globex = within(rendered[1]!);
    expect(globex.getByTestId("admin-workspace-row-letter")).toHaveTextContent("G");
    expect(globex.queryByTestId("admin-workspace-row-logo")).not.toBeInTheDocument();
    expect(globex.queryByTestId("admin-workspace-row-owner")).not.toBeInTheDocument();
    expect(globex.queryByTestId("admin-workspace-row-counts")).not.toBeInTheDocument();
  });

  it("falls back to the letter for an empty logo", async () => {
    const rows = [workspaceRow({ name: "Acme", logo_url: "" })];
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, workspacePage(rows, ""))) as Transport);
    renderList();
    await waitFor(() => expect(workspaceRows()).toHaveLength(1));
    expect(screen.getByTestId("admin-workspace-row-letter")).toHaveTextContent("A");
  });

  it("renders singular and zero counts honestly", async () => {
    const rows = [
      workspaceRow({ total_projects: 1, total_members: 1 }),
      workspaceRow({ total_projects: 0, total_members: 0 }),
    ];
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, workspacePage(rows, ""))) as Transport);
    renderList();
    await waitFor(() => expect(workspaceRows()).toHaveLength(2));
    const counts = screen.getAllByTestId("admin-workspace-row-counts");
    expect(counts[0]).toHaveTextContent("1 project · 1 member");
    expect(counts[1]).toHaveTextContent("0 projects · 0 members");
  });

  it("hides counts the server omits", async () => {
    const row = workspaceRow();
    delete row["total_projects"];
    delete row["total_members"];
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, workspacePage([row], ""))) as Transport);
    renderList();
    await waitFor(() => expect(workspaceRows()).toHaveLength(1));
    expect(screen.queryByTestId("admin-workspace-row-counts")).not.toBeInTheDocument();
  });

  it("deep-links every row into the main app in a new tab with no row actions", async () => {
    const rows = [workspaceRow({ slug: "acme" }), workspaceRow({ slug: "globex" })];
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, workspacePage(rows, ""))) as Transport);
    renderList();
    await waitFor(() => expect(workspaceRows()).toHaveLength(2));
    for (const row of workspaceRows()) {
      expect(row.tagName).toBe("A");
      expect(row).toHaveAttribute("target", "_blank");
      expect(row.getAttribute("rel")).toContain("noreferrer");
      expect(within(row).queryByRole("button")).not.toBeInTheDocument();
    }
    expect(workspaceRows()[0]).toHaveAttribute("href", "/acme");
    expect(workspaceRows()[1]).toHaveAttribute("href", "/globex");
  });

  it("shows a zero count with no rows and no illustration when empty", async () => {
    createAppClient("https://api.test", ((url: string) => jsonResponse(url, 200, workspacePage([], ""))) as Transport);
    renderList();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("0"));
    expect(screen.queryByTestId("admin-workspace-row")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-workspace-rows")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-helper")).toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-create")).toHaveAttribute("href", "/god-mode/workspace/create/");
  });

  it("appends the next page through load-more with a busy state and count spinner", async () => {
    const seen: string[] = [];
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const transport: Transport = (url) => {
      seen.push(url);
      const cursor = new URL(url, "https://api.test").searchParams.get("cursor");
      if (cursor === null) return jsonResponse(url, 200, workspacePage([workspaceRow({ slug: "acme" })], "cursor-2"));
      return gate.then(() => jsonResponse(url, 200, workspacePage([workspaceRow({ slug: "globex" })], "")));
    };
    createAppClient("https://api.test", transport);
    renderList();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("1"));
    const loadMore = screen.getByRole("button", { name: "Load more" });
    expect(loadMore).not.toBeDisabled();
    fireEvent.click(loadMore);
    await waitFor(() => expect(screen.getByRole("button", { name: "Load more" })).toBeDisabled());
    expect(screen.getByTestId("admin-workspace-count-spinner")).toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("2"));
    expect(workspaceRows()).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-workspace-count-spinner")).not.toBeInTheDocument();
    expect(seen).toHaveLength(2);
    expect(seen[0]).not.toContain("cursor=");
    expect(seen[1]).toContain("cursor=cursor-2");
  });

  it("offers a retry when the list fails to load", async () => {
    let calls = 0;
    const transport: Transport = (url) => {
      calls += 1;
      if (calls === 1) return jsonResponse(url, 500, { detail: "boom" });
      return jsonResponse(url, 200, workspacePage([workspaceRow({ slug: "acme" })], ""));
    };
    createAppClient("https://api.test", transport);
    renderList();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-list-error")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-workspace-count")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("admin-workspace-list-retry"));
    await waitFor(() => expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("1"));
    expect(calls).toBe(2);
  });
});
