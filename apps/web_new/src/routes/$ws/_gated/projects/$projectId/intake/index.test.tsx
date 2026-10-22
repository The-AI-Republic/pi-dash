// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake route (NEWFRONT-689, INT-001/INT-028). The route validates the
// two search keys, prefetches only when the project's intake is on, and
// mounts the intake-owned screen with the parsed state.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { createClient, type TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children }: { to: string; children?: React.ReactNode }) => <a href={to}>{children}</a>,
  useNavigate: () => vi.fn(),
}));

import { createAppClient } from "../../../../../../core/api/client.js";
import { Route } from "./index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface BeforeLoadArgs {
  context: { queryClient: QueryClient; apiClient: unknown };
  search: Record<string, unknown>;
  params: { ws: string; projectId: string };
}

function beforeLoad() {
  const fn = Route.options.beforeLoad as unknown as (args: BeforeLoadArgs) => Promise<void>;
  return fn;
}

function fakeApi(project: unknown, calls: string[]) {
  return createClient({
    baseUrl: "https://api.test",
    transport: (async (url: string) => {
      const parsed = new URL(url);
      calls.push(parsed.pathname + parsed.search);
      if (parsed.pathname.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (parsed.pathname === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 200, project);
      if (parsed.pathname.endsWith("/intake-issues/")) {
        return jsonResponse(url, 200, {
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
        });
      }
      return jsonResponse(url, 404, { error: "missing" });
    }) as never,
  });
}

const project = (inboxView: boolean) => ({
  id: PROJECT_ID,
  name: "Apollo",
  identifier: "APO",
  sort_order: null,
  logo_props: {},
  member_role: 15,
  archived_at: null,
  workspace: "123e4567-e89b-12d3-a456-426614174000",
  cycle_view: true,
  issue_views_view: true,
  module_view: true,
  page_view: false,
  inbox_view: inboxView,
  is_default: false,
  guest_view_all_features: false,
  project_lead: null,
  network: 0,
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
  agent_executor_options: [],
  default_agent_executor: "local_runner",
});

describe("intake route search schema (INT-001/INT-028)", () => {
  it("declares both keys and defaults to Open", () => {
    const validate = Route.options.validateSearch as unknown as (raw: Record<string, unknown>) => unknown;
    expect(validate({})).toEqual({ currentTab: "open" });
    expect(validate({ currentTab: "closed", inboxIssueId: "i-1" })).toEqual({
      currentTab: "closed",
      inboxIssueId: "i-1",
    });
    expect(validate({ currentTab: "nope" })).toEqual({ currentTab: "open" });
  });
});

describe("intake route prefetch (INT-002/INT-012)", () => {
  it("warms the active tab's first page while the feature is on", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    const apiClient = fakeApi(project(true), calls);
    await beforeLoad()({
      context: { queryClient, apiClient },
      search: { currentTab: "closed" },
      params: { ws: "acme", projectId: PROJECT_ID },
    });
    const list = calls.find((call) => call.includes("/intake-issues/"));
    expect(list).toBeDefined();
    expect(new URLSearchParams((list ?? "").split("?")[1] ?? "").get("status")).toBe("1,-1,2");
  });

  it("reads no list while the feature is off", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    const apiClient = fakeApi(project(false), calls);
    await beforeLoad()({
      context: { queryClient, apiClient },
      search: {},
      params: { ws: "acme", projectId: PROJECT_ID },
    });
    expect(calls.some((call) => call.includes("/intake-issues/"))).toBe(false);
  });

  it("does not fail the navigation when the project read is refused", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    const apiClient = fakeApi({ error: "nope" }, calls);
    await expect(
      beforeLoad()({ context: { queryClient, apiClient }, search: {}, params: { ws: "acme", projectId: PROJECT_ID } })
    ).resolves.toBeUndefined();
    expect(calls.some((call) => call.includes("/intake-issues/"))).toBe(false);
  });
});

describe("intake route component", () => {
  it("mounts the intake screen with the validated address state", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
    vi.spyOn(Route, "useSearch").mockReturnValue({ currentTab: "open" } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    createAppClient("https://api.test", (async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 200, project(true));
      if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
      if (path.endsWith("/api/users/me/workspaces/")) {
        return jsonResponse(url, 200, [
          {
            id: "123e4567-e89b-12d3-a456-426614174000",
            name: "Acme",
            slug: "acme",
            logo_url: null,
            total_members: 1,
            role: 20,
          },
        ]);
      }
      return jsonResponse(url, 200, {
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
      });
    }) as never);
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("intake-shell")).toBeInTheDocument());
    expect(await screen.findByTestId("intake-empty-open")).toBeInTheDocument();
  });
});
