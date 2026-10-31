// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues route (parity/specs/views.md §11, VIEW-038). The
// route declares the static default's address and mounts the workspace
// detail chunk with the static key: these cases pin the declaration, the
// warmed reads, and the committed screen. The screen itself (header,
// controls, scoped body) is covered in its own tests.
import { QueryClient } from "@tanstack/react-query";
import { createClient } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makeView } from "../../../../features/views/testing.js";
import { Route } from "./all-issues.js";

const navigateHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => navigateHarness.navigate,
}));

// The detail chunk stands in as an echo: the render case pins what the
// route itself owns (the static key, the params pass-through), while the
// screen's own suite owns the interior. `loadWorkspaceViewDetail` stays
// real so the warming test reads the genuine preload.
vi.mock("../../../../features/views/detail/lazy.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../features/views/detail/lazy.js")>()),
  resolvedWorkspaceViewDetail: () => ({
    WorkspaceViewScreen: function StubScreen({
      params,
      peek,
    }: {
      params: { ws: string; viewId: string };
      peek: { peekKey: string | null; peekProjectId: string | null; onPeekChange: (request: unknown) => void };
    }) {
      return (
        <div
          data-testid="stub-screen"
          data-peek={String(peek.peekKey ?? "")}
          data-peek-project={String(peek.peekProjectId ?? "")}
        >
          {`${params.ws}/${params.viewId}`}
          <button
            type="button"
            onClick={() => peek.onPeekChange({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 })}
          >
            open
          </button>
          <button type="button" onClick={() => peek.onPeekChange(null)}>
            close
          </button>
        </div>
      );
    },
  }),
}));

const ISSUE = "023e4567-e89b-12d3-a456-426614174010";
const PROJECT = "423e4567-e89b-12d3-a456-426614174003";

vi.mock("../../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/session/hooks.js")>()),
  usePermissions: () => ({ role: 15, isMember: true, canEdit: true, isAdmin: false, isLoading: false }),
  useMe: () => ({
    data: { id: "123e4567-e89b-12d3-a456-426614174000" },
    isPending: false,
    isError: false,
    refetch: () => Promise.resolve(),
  }),
  useWorkspace: () => ({ workspace: { name: "Acme", slug: "acme", role: 15 } }),
}));

const PARAMS = { ws: "acme" };

function emptyGlobalIssues() {
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
    id: "423e4567-e89b-12d3-a456-426614174003",
    name: "Website",
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

function handler(url: string): { status: number; body: unknown } {
  if (url.endsWith("/projects/")) return { status: 200, body: [projectRow()] };
  if (url.includes("/views/")) {
    return {
      status: 200,
      body: [makeView({ id: "223e4567-e89b-12d3-a456-426614174001", project: null, name: "Triage" })],
    };
  }
  if (url.includes("/user-properties/")) {
    return { status: 200, body: { filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} } };
  }
  if (url.includes("/issues/")) return { status: 200, body: emptyGlobalIssues() };
  return { status: 200, body: [] };
}

function contextFor() {
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

describe("all-issues route", () => {
  it("declares the workspace all-issues path under the gated layout", () => {
    expect(Route.update).toBeTypeOf("function");
    expect(Route.options.component).toBeTypeOf("function");
  });

  it("shows the list skeleton while the route loads", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("missing pending component");
    render(<Pending />);
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("warms the customs list and the plain first page for the static key", async () => {
    const { context, calls } = contextFor();
    await Route.options.beforeLoad?.({ context, params: PARAMS } as never);
    expect(calls[0]).toContain("/views/?");
    expect(calls[1]).toContain("/api/workspaces/acme/issues/?");
    expect(calls[1]).not.toContain("order_by=");
  });

  it("reads the projects list so the empty card can offer its create entry", async () => {
    const { context } = contextFor();
    const loader = Route.options.loader as unknown as (input: unknown) => Promise<unknown>;
    await loader({ context, params: PARAMS, deps: {} });
    expect(context.queryClient.getQueryData(["ws", PARAMS.ws, "projects", "list"])).toBeDefined();
  });

  it("renders the detail chunk with the static key", async () => {
    const paramsSpy = vi.spyOn(Route, "useParams").mockReturnValue(PARAMS as never);
    const searchSpy = vi.spyOn(Route, "useSearch").mockReturnValue({} as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(await screen.findByTestId("stub-screen")).toHaveTextContent("acme/all-issues");
    expect(paramsSpy).toHaveBeenCalled();
    paramsSpy.mockRestore();
    searchSpy.mockRestore();
  });

  it("validates the peek params out of the address (ISS-167)", () => {
    const validate = Route.options.validateSearch as unknown as (
      search: Record<string, unknown>
    ) => Record<string, unknown>;
    expect(validate({ peekIssueId: ISSUE, peekProjectId: PROJECT, other: "x" })).toEqual({
      peekIssueId: ISSUE,
      peekProjectId: PROJECT,
    });
    expect(validate({ peekIssueId: 7 })).toEqual({});
  });

  it("hands the route's peek address to the screen (ISS-167)", async () => {
    const paramsSpy = vi.spyOn(Route, "useParams").mockReturnValue(PARAMS as never);
    const searchSpy = vi
      .spyOn(Route, "useSearch")
      .mockReturnValue({ peekIssueId: ISSUE, peekProjectId: PROJECT } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    const screenNode = await screen.findByTestId("stub-screen");
    expect(screenNode).toHaveAttribute("data-peek", ISSUE);
    expect(screenNode).toHaveAttribute("data-peek-project", PROJECT);
    paramsSpy.mockRestore();
    searchSpy.mockRestore();
  });

  it("writes the peek with replace and always names the row's project (ISS-167)", async () => {
    const paramsSpy = vi.spyOn(Route, "useParams").mockReturnValue(PARAMS as never);
    const searchSpy = vi.spyOn(Route, "useSearch").mockReturnValue({ view: "triage" } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    fireEvent.click(await screen.findByRole("button", { name: "open" }));
    expect(navigateHarness.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { view: "triage", peekIssueId: ISSUE, peekProjectId: PROJECT },
      replace: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "close" }));
    expect(navigateHarness.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { view: "triage" },
      replace: true,
    });
    paramsSpy.mockRestore();
    searchSpy.mockRestore();
  });
});
