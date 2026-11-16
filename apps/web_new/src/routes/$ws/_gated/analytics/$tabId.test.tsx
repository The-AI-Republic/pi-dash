// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics tab address (parity/specs/analytics.md §§1, 4,
// ANL-001, ANL-004): validated scope search, project prefetch, and
// the screen with scope changes navigating back to the address.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the route wiring.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode; role?: string }) => (
      <a href={props.to} role={props.role}>
        {props.children}
      </a>
    ),
  };
});

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsTotals: vi.fn(), listProjects: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../../core/session/hooks.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../core/session/hooks.js")>();
  return {
    ...original,
    useWorkspace: (() => ({ workspace: { name: "Acme Corp" }, role: 20 })) as unknown as typeof original.useWorkspace,
    usePermissions: (() => ({
      role: 20,
      isMember: true,
      canEdit: true,
      isAdmin: true,
      isLoading: false,
    })) as unknown as typeof original.usePermissions,
  };
});

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { projectKeys } from "../../../../features/projects/index.js";

import { Route } from "./$tabId.js";

const WORKSPACE = "44444444-4444-4444-8444-444444444444";
const ALPHA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BETA = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function projectRow(id: string, name: string, identifier: string) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: WORKSPACE,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    created_by: null,
    updated_by: null,
  };
}

const PROJECTS = [projectRow(ALPHA, "Alpha", "ALP"), projectRow(BETA, "Beta", "BET")];

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("analytics tab search", () => {
  it("validates project_ids into a scope list", () => {
    const validateSearch = Route.options.validateSearch as unknown as (
      search: Record<string, unknown>
    ) => Record<string, unknown>;
    expect(validateSearch({ project_ids: "a,b" })).toEqual({ project_ids: ["a", "b"] });
    expect(validateSearch({})).toEqual({});
  });
});

describe("analytics tab loader", () => {
  it("prefetches the project list the shell decides on", async () => {
    contractMocks.listProjects.mockResolvedValue(PROJECTS);
    const apiClient = { stub: true } as unknown as ApiClient;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const loader = Route.options.loader as unknown as (args: {
      context: { queryClient: QueryClient; apiClient: ApiClient };
      params: { ws: string };
    }) => Promise<void>;
    await loader({ context: { queryClient, apiClient }, params: { ws: "acme" } });
    expect(queryClient.getQueryData(projectKeys.list("acme"))).toEqual(PROJECTS);
  });
});

describe("analytics tab pending and error", () => {
  it("renders the route pending state", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("analytics tab has no pending component");
    render(<Pending />);
    expect(screen.getByTestId("analytics-route-pending")).toBeInTheDocument();
  });

  it("renders the load error with a retry", () => {
    const ErrorView = Route.options.errorComponent;
    if (!ErrorView) throw new Error("analytics tab has no error component");
    const reset = vi.fn();
    render(<ErrorView error={new Error("boom")} reset={reset} />);
    expect(screen.getByText("Could not load analytics")).toBeInTheDocument();
    expect(screen.getByText("boom")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("falls back for non-error failures", () => {
    const ErrorView = Route.options.errorComponent;
    if (!ErrorView) throw new Error("analytics tab has no error component");
    render(<ErrorView error="nope" reset={() => {}} />);
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });
});

describe("analytics tab route", () => {
  function renderRoute(search: Record<string, unknown>): { navigate: ReturnType<typeof vi.fn> } {
    const navigate = vi.fn();
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", tabId: "overview" });
    vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
    vi.spyOn(Route, "useNavigate").mockReturnValue(navigate as never);
    contractMocks.listProjects.mockResolvedValue(PROJECTS);
    contractMocks.getAnalyticsTotals.mockResolvedValue({});
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        {(() => {
          const Component = Route.options.component;
          if (!Component) throw new Error("analytics tab has no component");
          return <Component />;
        })()}
      </QueryClientProvider>
    );
    return { navigate };
  }

  it("mounts the screen for the addressed tab", async () => {
    renderRoute({});
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(screen.getByTestId("analytics-panel")).toHaveAttribute("id", "analytics-panel-overview");
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("All projects");
  });

  it("navigates picker selections back to the address scope", async () => {
    const user = userEvent.setup();
    const { navigate } = renderRoute({});
    await waitFor(() => expect(screen.getByTestId("analytics-picker-trigger")).toBeInTheDocument());
    await user.click(screen.getByTestId("analytics-picker-trigger"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(screen.getByRole("option", { name: "Beta" }));
    expect(navigate).toHaveBeenCalledWith({ to: ".", search: { project_ids: [BETA] }, replace: true });
  });

  it("clears the scope parameter when the selection empties", async () => {
    const user = userEvent.setup();
    const { navigate } = renderRoute({ project_ids: [BETA] });
    await waitFor(() => expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("Beta"));
    await user.click(screen.getByTestId("analytics-picker-trigger"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(screen.getByRole("option", { name: "Beta" }));
    expect(navigate).toHaveBeenCalledWith({ to: ".", search: {}, replace: true });
  });
});
