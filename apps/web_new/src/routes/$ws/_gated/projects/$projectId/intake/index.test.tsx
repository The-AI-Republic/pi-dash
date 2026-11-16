// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake route (NEWFRONT-689, INT-001/INT-028). The route validates the
// two search keys, prefetches only when the project's intake is on, and
// mounts the intake-owned screen with the parsed state.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createClient, type TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { beforeAll, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children }: { to: string; children?: React.ReactNode }) => <a href={to}>{children}</a>,
  useNavigate: () => navigate,
}));

const { notifyMocks } = vi.hoisted(() => ({
  notifyMocks: { notifyError: vi.fn(), notifySuccess: vi.fn() },
}));

vi.mock("../../../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../../../shared/shell/index.js")>()),
  ...notifyMocks,
}));

vi.mock("../../../../../../shared/editor/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../../../shared/editor/index.js")>();
  function StandInEditor({
    onChange,
    label,
    initialHtml,
  }: {
    onChange: (html: string) => void;
    label: string;
    initialHtml: string;
  }) {
    const [value, setValue] = React.useState(() => initialHtml.replace(/^<p>/, "").replace(/<\/p>$/, ""));
    return (
      <textarea
        aria-label={label}
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
          onChange(`<p>${event.target.value}</p>`);
        }}
      />
    );
  }
  return { ...original, IssueDescriptionEditor: StandInEditor };
});

import { createAppClient } from "../../../../../../core/api/client.js";
import { loadIntakeRouteChunk } from "../../../../../../features/intake/route/lazy.js";
import { Route } from "./index.js";
import { Route as InboxRoute } from "../inbox.js";

// The route component renders the resolved chunk (NEWFRONT-869), which
// production guarantees via beforeLoad. Await it once up front so the
// component tests do not depend on the prefetch tests having run first.
beforeAll(async () => {
  await loadIntakeRouteChunk();
});

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

  it("asks the router to push user moves onto the address", async () => {
    navigate.mockReset();
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
    await screen.findByTestId("intake-shell");
    fireEvent.click(screen.getByRole("tab", { name: /Closed/ }));
    expect(navigate).toHaveBeenCalledWith({ to: ".", search: { currentTab: "closed" }, replace: false });
  });

  it("asks the router to replace the address with the screen's settle correction", async () => {
    navigate.mockReset();
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
    vi.spyOn(Route, "useSearch").mockReturnValue({ currentTab: "open", inboxIssueId: "stale-id" } as never);
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
    await screen.findByTestId("intake-shell");
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: ".", search: { currentTab: "open" }, replace: true })
    );
  });
});

describe("intake route create dialog (INT-013/INT-015)", () => {
  const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";

  function createdResponse(name: string) {
    return {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
      status: -2,
      duplicate_to: null,
      snoozed_till: null,
      duplicate_issue_detail: null,
      source: "IN_APP",
      issue: {
        id: ISSUE_ID,
        name,
        description_html: "",
        priority: "none",
        state_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        sequence_id: 7,
        project_id: PROJECT_ID,
        parent_id: null,
        label_ids: [],
        assignee_ids: [],
        target_date: null,
        start_date: null,
        estimate_point: null,
        sort_order: 1,
        completed_at: null,
        created_at: "2026-10-23T00:00:00Z",
        updated_at: "2026-10-23T00:00:00Z",
        created_by: null,
        updated_by: null,
        is_draft: false,
        archived_at: null,
        complexity_score: 0,
        agent_executor: null,
        is_synced: false,
        is_subscribed: false,
        agent_ticker: null,
        agent_status: null,
        relations_summary: { blocked_by: [], blocking: [] },
        has_open_blockers: false,
      },
    };
  }

  function renderRoute() {
    navigate.mockReset();
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
    vi.spyOn(Route, "useSearch").mockReturnValue({ currentTab: "open" } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
      const method = init.method ?? "GET";
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
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/intake-issues/` && method === "POST") {
        const name = (JSON.parse(init.body ?? "{}") as { issue?: { name?: string } }).issue?.name ?? "";
        return jsonResponse(url, 200, createdResponse(name));
      }
      if (path.endsWith("/api/instances/")) return jsonResponse(url, 404, { error: "missing" });
      if (path.endsWith("/api/users/me/profile/")) return jsonResponse(url, 404, { error: "missing" });
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
  }

  it("opens the dialog from the header control", async () => {
    renderRoute();
    await screen.findByTestId("intake-shell");
    fireEvent.click(await screen.findByTestId("intake-header-create"));
    expect(await screen.findByRole("heading", { name: "New intake request" })).toBeInTheDocument();
  });

  it("moves to the new request on the Open tab after a save", async () => {
    renderRoute();
    await screen.findByTestId("intake-shell");
    fireEvent.click(await screen.findByTestId("intake-header-create"));
    await screen.findByRole("heading", { name: "New intake request" });
    fireEvent.change(screen.getByTestId("intake-create-title"), { target: { value: "Triage me" } });
    fireEvent.click(screen.getByTestId("intake-create-submit"));
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());
    expect(navigate).toHaveBeenCalledWith({
      to: ".",
      search: { currentTab: "open", inboxIssueId: ISSUE_ID },
      replace: false,
    });
  });
});

describe("retired project inbox route (INT-001)", () => {
  it("redirects the old address onto the canonical intake screen", async () => {
    const beforeLoad = InboxRoute.options.beforeLoad as unknown as (args: {
      params: { ws: string; projectId: string };
    }) => Promise<void>;
    // The forwarder's module rides a dynamic import (NEWFRONT-871, size
    // gate), so the redirect now rejects the returned promise.
    await expect(beforeLoad({ params: { ws: "acme", projectId: PROJECT_ID } })).rejects.toMatchObject({
      options: { href: `/acme/projects/${PROJECT_ID}/intake`, statusCode: 307 },
    });
  });
});
