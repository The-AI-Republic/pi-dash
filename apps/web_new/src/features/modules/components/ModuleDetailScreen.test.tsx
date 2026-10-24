// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail screen frames the shared host with the trail, the tab
// title, the badge and toggle header actions, and the collapsible
// sidebar beside the layouts (MOD-029, MOD-030, MOD-032).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  projects: vi.fn(),
  detail: vi.fn(),
  prefs: vi.fn(),
  page: vi.fn(),
  peekChange: vi.fn(),
}));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useSuspenseQuery: (options: { queryKey: readonly unknown[] }) => {
    const client = (globalThis as { __detailQueryClient?: QueryClient }).__detailQueryClient;
    return { data: client?.getQueryData(options.queryKey as never) };
  },
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("../../projects/index.js", () => ({ projectsQueryOptions: mocks.projects }));
vi.mock("./sidebar/moduleSidebarQueries.js", () => ({ moduleDetailQueryOptions: mocks.detail }));
vi.mock("../../issues/prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("../../issues/list/pages.js", () => ({ listPageQueryOptions: mocks.page }));

vi.mock("./ModuleDetailCrumbs.js", () => ({
  ModuleDetailCrumbs: (props: { projectName: string; moduleName: string }) => (
    <div data-testid="crumbs-stub" data-project={props.projectName} data-module={props.moduleName} />
  ),
}));

vi.mock("../../issues/list/EntityListScreen.js", () => ({
  EntityListScreen: (props: {
    entity: unknown;
    title: string;
    moduleId: string;
    completedModule: boolean;
    peekKey: string | null;
    compact: boolean;
    extraHeaderActions: React.ReactNode;
    onPeekChange: (key: string | null) => void;
  }) => (
    <div
      data-testid="entity-stub"
      data-entity={JSON.stringify(props.entity)}
      data-title={props.title}
      data-module={props.moduleId}
      data-module-completed={String(props.completedModule)}
      data-peek={String(props.peekKey ?? "")}
      data-compact={String(props.compact)}
    >
      <button type="button" onClick={() => props.onPeekChange("WEB-1")}>
        peek
      </button>
      <div data-testid="extra-actions">{props.extraHeaderActions}</div>
    </div>
  ),
}));

vi.mock("./sidebar/ModuleAnalyticsSidebar.js", () => ({
  ModuleAnalyticsSidebar: (props: { moduleId: string; onClose?: () => void }) => (
    <div data-testid="sidebar-stub" data-module={props.moduleId}>
      {props.onClose === undefined ? null : (
        <button type="button" data-testid="sidebar-close" onClick={props.onClose}>
          close
        </button>
      )}
    </div>
  ),
}));

import { ModuleDetailScreen } from "./ModuleDetailScreen.js";
import { MODULE_DETAIL_SIDEBAR_STORAGE_KEY } from "../lib/detailSidebarStore.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const PROJECTS_KEY = ["projects"] as const;
const DETAIL_KEY = ["detail"] as const;
const PREFS_KEY = ["prefs"] as const;
const PAGE_KEY = ["page"] as const;

function project(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    member_role: 20,
    archived_at: null,
    cycle_view: true,
    module_view: true,
    ...overrides,
  };
}

function detail(overrides: Record<string, unknown> = {}) {
  return {
    id: MODULE,
    project_id: PROJECT,
    name: "Delivery",
    status: "in-progress",
    ...overrides,
  };
}

function setup(options: { projects?: Array<Record<string, unknown>>; total?: number; status?: string } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  (globalThis as { __detailQueryClient?: QueryClient }).__detailQueryClient = queryClient;
  queryClient.setQueryData(PROJECTS_KEY, options.projects ?? [project()]);
  queryClient.setQueryData(DETAIL_KEY, detail(options.status === undefined ? {} : { status: options.status }));
  queryClient.setQueryData(PREFS_KEY, {
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
  });
  queryClient.setQueryData(PAGE_KEY, { total_count: options.total ?? 7, results: [], count: 0 });
  mocks.peekChange.mockClear();
  return render(
    <QueryClientProvider client={queryClient}>
      <ModuleDetailScreen
        workspaceSlug="acme"
        projectId={PROJECT}
        moduleId={MODULE}
        peekKey={null}
        onPeekChange={mocks.peekChange}
        compact={false}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  globalThis.localStorage.clear();
  document.title = "";
  mocks.projects.mockReturnValue({ queryKey: PROJECTS_KEY, queryFn: vi.fn() });
  mocks.detail.mockReturnValue({ queryKey: DETAIL_KEY, queryFn: vi.fn() });
  mocks.prefs.mockReturnValue({ queryKey: PREFS_KEY, queryFn: vi.fn() });
  mocks.page.mockReturnValue({ queryKey: PAGE_KEY, queryFn: vi.fn() });
});

describe("ModuleDetailScreen", () => {
  it("frames the host with the trail, title, badge and sidebar", () => {
    setup();
    expect(screen.getByTestId("module-detail-screen")).toBeInTheDocument();
    const crumbs = screen.getByTestId("crumbs-stub");
    expect(crumbs).toHaveAttribute("data-project", "Website");
    expect(crumbs).toHaveAttribute("data-module", "Delivery");
    expect(document.title).toBe("Website - Delivery");
    const host = screen.getByTestId("entity-stub");
    expect(host).toHaveAttribute(
      "data-entity",
      JSON.stringify({ scope: "module", projectId: PROJECT, moduleId: MODULE })
    );
    expect(host).toHaveAttribute("data-title", "Delivery");
    expect(host).toHaveAttribute("data-module", MODULE);
    expect(host).toHaveAttribute("data-module-completed", "false");
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).getByTestId("module-detail-badge")).toHaveTextContent("7");
    expect(within(actions).getByTestId("module-detail-sidebar-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-module", MODULE);
  });

  it("reads the badge from the unfiltered first page", () => {
    setup();
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: true,
    });
  });

  it("hides the badge on an empty module", () => {
    setup({ total: 0 });
    expect(screen.queryByTestId("module-detail-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toBeInTheDocument();
  });

  it("marks a completed module read-only for the host", () => {
    setup({ status: "completed" });
    expect(screen.getByTestId("entity-stub")).toHaveAttribute("data-module-completed", "true");
  });

  it("forwards peek changes to the route", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "peek" }));
    expect(mocks.peekChange).toHaveBeenCalledWith("WEB-1");
  });

  it("collapses and expands the sidebar through the header toggle, persisting the choice", () => {
    setup();
    const toggle = screen.getByTestId("module-detail-sidebar-toggle");
    expect(toggle).toHaveAccessibleName("Collapse sidebar");
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe(null);
    fireEvent.click(toggle);
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toHaveAccessibleName("Expand sidebar");
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("true");
    fireEvent.click(screen.getByTestId("module-detail-sidebar-toggle"));
    expect(screen.getByTestId("sidebar-stub")).toBeInTheDocument();
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("false");
  });

  it("restores a collapsed sidebar as hidden", () => {
    globalThis.localStorage.setItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY, "true");
    setup();
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toHaveAccessibleName("Expand sidebar");
  });

  it("collapses through the sidebar's own close control", () => {
    setup();
    fireEvent.click(screen.getByTestId("sidebar-close"));
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("true");
  });

  it("renders the project-not-found state when the project is missing", () => {
    setup({ projects: [] });
    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.queryByTestId("entity-stub")).not.toBeInTheDocument();
    expect(document.title).toBe("Modules");
  });
});
