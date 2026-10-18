// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  projects: vi.fn(),
  listPage: vi.fn(),
  prefs: vi.fn(),
}));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useSuspenseQuery: (options: { queryKey: readonly unknown[] }) => {
    const client = (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient;
    return { data: client?.getQueryData(options.queryKey as never) };
  },
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("../../projects/index.js", () => ({ projectsQueryOptions: mocks.projects }));
vi.mock("./pages.js", () => ({ listPageQueryOptions: mocks.listPage }));
vi.mock("../prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));

vi.mock("../display/DisplayControl.js", () => ({
  DisplayControl: (props: { hideCycle: boolean; hideModule: boolean; offered: readonly string[] }) => (
    <div data-testid="display-control" data-cycle={String(props.hideCycle)} data-module={String(props.hideModule)}>
      {props.offered.join(",")}
    </div>
  ),
}));

vi.mock("../layouts/IssuesLayoutRoot.js", () => ({
  IssuesLayoutRoot: (props: {
    title: string;
    totalCount: number;
    resultCount: number;
    offered: readonly string[];
    renderLayout: (layout: string, prefs: Record<string, unknown>) => React.ReactNode;
    emptyState: React.ReactNode;
    noMatchState: React.ReactNode;
    headerActions: React.ReactNode;
    displayControl: React.ReactNode;
  }) => (
    <div
      data-testid="layout-root"
      data-title={props.title}
      data-total={props.totalCount}
      data-results={props.resultCount}
      data-offered={props.offered.join(",")}
    >
      <div data-testid="header-actions">{props.headerActions}</div>
      <div data-testid="display-slot">{props.displayControl}</div>
      <div data-testid="empty-state">{props.emptyState}</div>
      <div data-testid="no-match-state">{props.noMatchState}</div>
      <div data-testid="layout">{props.renderLayout("list", { layout: "list", groupBy: null })}</div>
    </div>
  ),
}));

vi.mock("./ListLayout.js", () => ({
  ListLayout: (props: Record<string, unknown>) => (
    <div
      data-testid="list-layout"
      data-cycle={String(props.cycleId ?? "")}
      data-module={String(props.moduleId ?? "")}
      data-completed={String(props.completedCycle ?? false)}
      data-compact={String(props.compact ?? false)}
      data-project={String(props.projectId)}
    >
      {props.projectIdentifier as string}
    </div>
  ),
}));

import { EntityListScreen, type EntityListScreenProps } from "./EntityListScreen.js";
import type { IssuesEntity } from "../prefs/entities.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";
const PROJECTS_KEY = ["projects"] as const;
const PAGE_KEY = ["page"] as const;
const PREFS_KEY = ["prefs"] as const;

function project(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    cycle_view: true,
    module_view: true,
    ...overrides,
  };
}

function setup(overrides: Partial<EntityListScreenProps> = {}, projects: Array<Record<string, unknown>> = [project()]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
  queryClient.setQueryData(PROJECTS_KEY, projects);
  queryClient.setQueryData(PREFS_KEY, { filters: {}, rich_filters: { priority__in: "urgent" } });
  queryClient.setQueryData(PAGE_KEY, { total_count: 7, results: [{ id: "one" }, { id: "two" }], count: 2 });
  const props: EntityListScreenProps = {
    workspaceSlug: "acme",
    projectId: PROJECT,
    entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE } as IssuesEntity,
    title: "Sprint 1",
    cycleId: CYCLE,
    peekKey: "WEB-42",
    onPeekChange: vi.fn(),
    compact: true,
    ...overrides,
  };
  return {
    props,
    view: render(
      <QueryClientProvider client={queryClient}>
        <EntityListScreen {...props} />
      </QueryClientProvider>
    ),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.projects.mockReturnValue({ queryKey: PROJECTS_KEY, queryFn: vi.fn() });
  mocks.listPage.mockReturnValue({ queryKey: PAGE_KEY, queryFn: vi.fn() });
  mocks.prefs.mockReturnValue({ queryKey: PREFS_KEY, queryFn: vi.fn() });
});

describe("EntityListScreen", () => {
  it("mounts the shared layout root with counts, list-only and the entity", async () => {
    setup();
    const root = await screen.findByTestId("layout-root");
    expect(root).toHaveAttribute("data-title", "Sprint 1");
    expect(root).toHaveAttribute("data-total", "7");
    expect(root).toHaveAttribute("data-results", "2");
    expect(root).toHaveAttribute("data-offered", "list");
    expect(await screen.findByTestId("list-layout")).toHaveTextContent("WEB");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-cycle", CYCLE);
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-compact", "true");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-completed", "false");
  });

  it("reads the flat first page for the entity", () => {
    setup();
    expect(mocks.listPage).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE },
      groupBy: null,
      orderBy: "sort_order",
      richFilters: { priority__in: "urgent" },
    });
  });

  it("hides cycle and module options the project does not offer", () => {
    setup({}, [project({ cycle_view: false, module_view: false })]);
    const controls = screen.getAllByTestId("display-control");
    expect(controls).toHaveLength(2);
    for (const control of controls) {
      expect(control).toHaveAttribute("data-cycle", "true");
      expect(control).toHaveAttribute("data-module", "true");
    }
  });

  it("renders the completed and module context", () => {
    setup({
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      moduleId: MODULE,
      completedCycle: true,
    });
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-module", MODULE);
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-completed", "true");
  });

  it("falls back to a count envelope when results are not an array", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
    queryClient.setQueryData(PROJECTS_KEY, [project()]);
    queryClient.setQueryData(PREFS_KEY, { filters: {}, rich_filters: {} });
    queryClient.setQueryData(PAGE_KEY, { total_count: 5, results: {}, count: 5 });
    render(
      <QueryClientProvider client={queryClient}>
        <EntityListScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          entity={{ scope: "module", projectId: PROJECT, moduleId: MODULE }}
          title="Auth"
          moduleId={MODULE}
          peekKey={null}
          onPeekChange={vi.fn()}
          compact={false}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-results", "5");
  });

  it("renders the project-not-found state when the project is missing", () => {
    setup({}, []);
    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.queryByTestId("layout-root")).not.toBeInTheDocument();
  });

  it("wires the empty states to the entity title", () => {
    setup();
    expect(screen.getByTestId("empty-state")).toHaveTextContent("No work items in Sprint 1");
    expect(screen.getByTestId("no-match-state")).toHaveTextContent("No work items in Sprint 1");
  });
});
