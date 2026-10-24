// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail route (MOD-029, MOD-030, MOD-032): the gone-away gate
// around the lazy screen, peek and compact search wiring, the search
// validation, and the data and chunk prefetch with its 404 tolerance.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  page: vi.fn(),
  prefs: vi.fn(),
  projects: vi.fn(),
  detail: vi.fn(),
  loadKanban: vi.fn(async () => ({ default: () => null })),
  loadLeaves: vi.fn(async () => undefined),
  primeLeaves: vi.fn(),
  preloadFilters: vi.fn(),
}));

vi.mock("../../../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));
vi.mock("../../../../../../features/issues/prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("../../../../../../features/issues/list/pages.js", () => ({ listPageQueryOptions: mocks.page }));
vi.mock("../../../../../../features/projects/index.js", () => ({ projectsQueryOptions: mocks.projects }));
vi.mock("../../../../../../features/modules/components/sidebar/moduleSidebarQueries.js", () => ({
  moduleDetailQueryOptions: mocks.detail,
}));
vi.mock("../../../../../../features/issues/kanban/lazy.js", () => ({ loadKanbanLayout: mocks.loadKanban }));
vi.mock("../../../../../../features/issues/components/lazyLeaves.js", () => ({
  loadIssueListLeaves: mocks.loadLeaves,
  primeIssueListLeaves: mocks.primeLeaves,
}));
vi.mock("../../../../../../features/issues/filters/rich/lazy.js", () => ({
  preloadIssueFilters: mocks.preloadFilters,
}));
vi.mock("../../../../../../features/issues/list/lazy.js", () => ({
  loadListLayout: async () => ({ default: () => null }),
}));
vi.mock("../../../../../../features/modules/components/ModuleDetailGate.js", () => ({
  ModuleDetailGate: (props: Record<string, unknown>) => (
    <div
      data-testid="detail-gate"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
      data-module={String(props.moduleId ?? "")}
    >
      {props.children as never}
    </div>
  ),
}));
vi.mock("../../../../../../features/modules/components/ModuleDetailScreen.js", () => ({
  ModuleDetailScreen: (props: Record<string, unknown>) => (
    <div
      data-testid="detail-screen"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
      data-module={String(props.moduleId ?? "")}
      data-peek={String(props.peekKey ?? "")}
      data-compact={String(props.compact ?? false)}
    >
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)("WEB-1")}>
        open
      </button>
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)(null)}>
        close
      </button>
    </div>
  ),
}));
import { Route } from "./$moduleId.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

function renderRoute(search: Record<string, unknown> = {}) {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT, moduleId: MODULE } as never);
  vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      {React.createElement(Route.options.component as React.ComponentType) as never}
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.page.mockReturnValue({
    queryKey: ["page"],
    queryFn: async () => ({
      total_count: 0,
      results: [],
      count: 0,
      grouped_by: null,
      sub_grouped_by: null,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      total_results: 0,
    }),
  });
  mocks.prefs.mockReturnValue({
    queryKey: ["prefs"],
    queryFn: async () => ({ filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} }),
  });
  mocks.projects.mockReturnValue({ queryKey: ["projects"], queryFn: async () => [] });
  mocks.detail.mockReturnValue({ queryKey: ["detail"], queryFn: async () => ({ id: MODULE }) });
});

describe("module detail route", () => {
  it("mounts the screen behind the gone-away gate", async () => {
    renderRoute();
    const gate = await screen.findByTestId("detail-gate");
    expect(gate).toHaveAttribute("data-workspace", "acme");
    expect(gate).toHaveAttribute("data-project", PROJECT);
    expect(gate).toHaveAttribute("data-module", MODULE);
    const node = await screen.findByTestId("detail-screen");
    expect(node).toHaveAttribute("data-workspace", "acme");
    expect(node).toHaveAttribute("data-project", PROJECT);
    expect(node).toHaveAttribute("data-module", MODULE);
    expect(node).toHaveAttribute("data-peek", "");
    expect(node).toHaveAttribute("data-compact", "false");
  });

  it("passes peek and compact from the search state", async () => {
    renderRoute({ peekIssueId: "WEB-7", layout: "compact" });
    const screenNode = await screen.findByTestId("detail-screen");
    expect(screenNode).toHaveAttribute("data-peek", "WEB-7");
    expect(screenNode).toHaveAttribute("data-compact", "true");
  });

  it("writes and clears the peek param with replace", async () => {
    renderRoute({ peekIssueId: "WEB-7", peekProjectId: "project-9", peekNestingLevel: "1", layout: "compact" });
    fireEvent.click(await screen.findByRole("button", { name: "open" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { peekIssueId: "WEB-1", layout: "compact" },
      replace: true,
    });
    fireEvent.click(await screen.findByRole("button", { name: "close" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { layout: "compact" },
      replace: true,
    });
  });

  it("prefetches the module list page and preferences with the module entity", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await beforeLoad({
      context: { queryClient, apiClient: { stub: true } },
      params: { ws: "acme", projectId: PROJECT, moduleId: MODULE },
    } as never);
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: true,
    });
    expect(mocks.prefs).toHaveBeenCalledWith(expect.anything(), "acme", {
      scope: "module",
      projectId: PROJECT,
      moduleId: MODULE,
    });
  }, 20000);

  it("ensures the project list and the module detail with the layout chunks", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await beforeLoad({
      context: { queryClient, apiClient: { stub: true } },
      params: { ws: "acme", projectId: PROJECT, moduleId: MODULE },
    } as never);
    expect(mocks.projects).toHaveBeenCalledWith(expect.anything(), "acme");
    expect(mocks.detail).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, MODULE);
    // The shared prefetch starts the board chunk too (NEWFRONT-623), so
    // the route asserts requested, not once — the loader dedupes.
    expect(mocks.loadKanban).toHaveBeenCalled();
    expect(mocks.loadLeaves).toHaveBeenCalledTimes(1);
  }, 20000);

  it("lets a 404 fall through to the gone-away gate", async () => {
    mocks.prefs.mockReturnValue({
      queryKey: ["prefs"],
      queryFn: async () => {
        throw new ApiError({ status: 404, code: "http", message: "missing" });
      },
    });
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, moduleId: MODULE },
      } as never)
    ).resolves.toBe(undefined);
  }, 20000);

  it("rethrows a non-404 prefetch failure", async () => {
    mocks.prefs.mockReturnValue({
      queryKey: ["prefs"],
      queryFn: async () => {
        throw new ApiError({ status: 500, code: "http", message: "boom" });
      },
    });
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, moduleId: MODULE },
      } as never)
    ).rejects.toThrow("boom");
  }, 20000);

  it("renders the list skeleton while pending", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("missing pendingComponent");
    render(React.createElement(Pending));
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("renders the load error with a retry entry", () => {
    const ErrorView = Route.options.errorComponent;
    if (!ErrorView) throw new Error("missing errorComponent");
    const reset = vi.fn();
    render(React.createElement(ErrorView as React.ComponentType, { error: new Error("gone"), reset } as never));
    expect(screen.getByRole("heading", { name: "Could not load module details" })).toBeInTheDocument();
    expect(screen.getByText("gone")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("falls back to a generic message for a non-error failure", () => {
    const ErrorView = Route.options.errorComponent;
    if (!ErrorView) throw new Error("missing errorComponent");
    render(React.createElement(ErrorView as React.ComponentType, { error: "nope", reset: vi.fn() } as never));
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });

  it("validates the search through the shared issue search", () => {
    const validate = Route.options.validateSearch as (search: Record<string, unknown>) => unknown;
    expect(validate({})).toBeDefined();
    expect(validate({ peekIssueId: "WEB-1", layout: "compact" })).toMatchObject({
      peekIssueId: "WEB-1",
      layout: "compact",
    });
  });
});
