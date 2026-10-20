// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  projects: vi.fn(),
  listPage: vi.fn(),
  prefs: vi.fn(),
  pickerCycles: vi.fn(),
  pickerModules: vi.fn(),
}));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useSuspenseQuery: (options: { queryKey: readonly unknown[] }) => {
    const client = (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient;
    return { data: client?.getQueryData(options.queryKey as never) };
  },
  useQuery: (options: { queryKey: readonly unknown[]; enabled?: boolean }) => {
    const client = (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient;
    return {
      data: options.enabled === false ? undefined : client?.getQueryData(options.queryKey as never),
    };
  },
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("../../projects/index.js", () => ({ projectsQueryOptions: mocks.projects }));
vi.mock("./pages.js", () => ({ listPageQueryOptions: mocks.listPage }));
vi.mock("../prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("../../../shared/pickers/reference.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/pickers/reference.js")>()),
  pickerCyclesQueryOptions: mocks.pickerCycles,
  pickerModulesQueryOptions: mocks.pickerModules,
}));

vi.mock("../calendar/CalendarEntry.js", () => ({
  CalendarEntry: (props: Record<string, unknown>) => (
    <div
      data-testid="calendar-entry"
      data-entity={JSON.stringify(props.entity)}
      data-can-edit={String(props.canEdit)}
      data-membership={JSON.stringify(props.membership ?? {})}
    />
  ),
}));

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
    filterRow?: React.ReactNode;
  }) => (
    <div
      data-testid="layout-root"
      data-title={props.title}
      data-total={props.totalCount}
      data-results={props.resultCount}
      data-offered={props.offered.join(",")}
    >
      <div data-testid="header-actions">{props.headerActions}</div>
      <div data-testid="banner">{props.filterRow}</div>
      <div data-testid="display-slot">{props.displayControl}</div>
      <div data-testid="empty-state">{props.emptyState}</div>
      <div data-testid="no-match-state">{props.noMatchState}</div>
      <div data-testid="layout">{props.renderLayout("list", { layout: "list", groupBy: null })}</div>
      <div data-testid="calendar-layout">{props.renderLayout("calendar", { layout: "calendar", groupBy: null })}</div>
    </div>
  ),
}));

const layout = vi.hoisted(() => ({ resolved: true }));

vi.mock("./lazy.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lazy.js")>();
  return {
    ...actual,
    resolvedListLayout: () => {
      if (!layout.resolved) return actual.resolvedListLayout();
      return function ResolvedListLayout(props: Record<string, unknown>) {
        return (
          <div
            data-testid="list-layout"
            data-cycle={String(props.cycleId ?? "")}
            data-module={String(props.moduleId ?? "")}
            data-completed={String(props.completedCycle ?? false)}
            data-module-completed={String(props.completedModule ?? false)}
            data-compact={String(props.compact ?? false)}
            data-project={String(props.projectId)}
            data-can-edit={String(props.canEdit ?? "unset")}
          >
            {props.projectIdentifier as string}
          </div>
        );
      };
    },
  };
});

import { EntityListScreen, type EntityListScreenProps } from "./EntityListScreen.js";
import { COMPLETED_CYCLE_NOTE } from "../transfer/TransferIssuesPrompt.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { transferKeys } from "../transfer/api.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";
const PROJECTS_KEY = ["projects"] as const;
const PAGE_KEY = ["page"] as const;
const PREFS_KEY = ["prefs"] as const;
const CYCLES_KEY = ["picker", "cycles"] as const;
const MODULES_KEY = ["picker", "modules"] as const;

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

function setup(overrides: Partial<EntityListScreenProps> = {}, projects: Array<Record<string, unknown>> = [project()]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
  queryClient.setQueryData(PROJECTS_KEY, projects);
  queryClient.setQueryData(PREFS_KEY, {
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: { priority__in: "urgent" },
  });
  queryClient.setQueryData(PAGE_KEY, { total_count: 7, results: [{ id: "one" }, { id: "two" }], count: 2 });
  queryClient.setQueryData(CYCLES_KEY, [{ id: CYCLE, name: "Sprint 1", status: "ACTIVE" }]);
  queryClient.setQueryData(MODULES_KEY, [{ id: MODULE, name: "Auth", status: "ACTIVE" }]);
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

/** Seed the cycle detail the prompt reads, then repaint the screen. */
function setCycleDetail(
  harness: { props: EntityListScreenProps; view: ReturnType<typeof render> },
  detail: Record<string, unknown>
): void {
  const client = (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient as QueryClient;
  client.setQueryData(transferKeys.cycle("acme", PROJECT, CYCLE), {
    id: CYCLE,
    project_id: PROJECT,
    name: "Sprint 1",
    start_date: null,
    end_date: null,
    ...detail,
  });
  harness.view.rerender(
    <QueryClientProvider client={client}>
      <EntityListScreen {...harness.props} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.projects.mockReturnValue({ queryKey: PROJECTS_KEY, queryFn: vi.fn() });
  mocks.listPage.mockReturnValue({ queryKey: PAGE_KEY, queryFn: vi.fn() });
  mocks.prefs.mockReturnValue({ queryKey: PREFS_KEY, queryFn: vi.fn() });
  mocks.pickerCycles.mockReturnValue({ queryKey: CYCLES_KEY, queryFn: vi.fn() });
  mocks.pickerModules.mockReturnValue({ queryKey: MODULES_KEY, queryFn: vi.fn() });
});

describe("EntityListScreen", () => {
  it("mounts the shared layout root with counts, list-only and the entity", async () => {
    setup();
    const root = await screen.findByTestId("layout-root");
    expect(root).toHaveAttribute("data-title", "Sprint 1");
    expect(root).toHaveAttribute("data-total", "7");
    expect(root).toHaveAttribute("data-results", "2");
    expect(root).toHaveAttribute("data-offered", "list,calendar");
    expect(await screen.findByTestId("list-layout")).toHaveTextContent("WEB");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-cycle", CYCLE);
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-compact", "true");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-completed", "false");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-can-edit", "true");
  });

  it("resolves the cycle title and completed state from the picker reference", () => {
    setup({
      entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE },
      cycleId: CYCLE,
      title: undefined,
    });
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-title", "Sprint 1");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-completed", "false");
  });

  it("resolves the module title from the picker reference", () => {
    setup({
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      moduleId: MODULE,
      title: undefined,
    });
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-title", "Auth");
  });

  it("falls back to the generic title when the entity row is absent", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
    queryClient.setQueryData(PROJECTS_KEY, [project()]);
    queryClient.setQueryData(PREFS_KEY, { filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} });
    queryClient.setQueryData(PAGE_KEY, { total_count: 0, results: [], count: 0 });
    queryClient.setQueryData(CYCLES_KEY, []);
    render(
      <QueryClientProvider client={queryClient}>
        <EntityListScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          entity={{ scope: "cycle", projectId: PROJECT, cycleId: CYCLE }}
          cycleId={CYCLE}
          peekKey={null}
          onPeekChange={vi.fn()}
          compact={false}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-title", "Cycle");
  });

  it("mounts the calendar with the entity and its cycle membership", () => {
    setup();
    const calendar = screen.getByTestId("calendar-entry");
    expect(calendar).toHaveAttribute(
      "data-entity",
      JSON.stringify({ scope: "cycle", projectId: PROJECT, cycleId: CYCLE })
    );
    expect(calendar).toHaveAttribute("data-membership", JSON.stringify({ cycleId: CYCLE }));
    expect(calendar).toHaveAttribute("data-can-edit", "true");
  });

  it("mounts the module calendar read-only for a completed context", () => {
    setup({
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      moduleId: MODULE,
      completedModule: true,
    });
    const calendar = screen.getByTestId("calendar-entry");
    expect(calendar).toHaveAttribute("data-membership", JSON.stringify({ moduleId: MODULE }));
    expect(calendar).toHaveAttribute("data-can-edit", "false");
  });

  it("reads the flat first page for the entity", () => {
    setup();
    expect(mocks.listPage).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE },
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: true,
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
      completedModule: true,
    });
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-module", MODULE);
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-module-completed", "true");
  });

  it("forwards a completed module context to the list", () => {
    setup({
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      moduleId: MODULE,
      completedModule: true,
    });
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-module-completed", "true");
  });

  it("derives a completed module from its picker row", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
    queryClient.setQueryData(PROJECTS_KEY, [project()]);
    queryClient.setQueryData(PREFS_KEY, { filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} });
    queryClient.setQueryData(PAGE_KEY, { total_count: 0, results: [], count: 0 });
    queryClient.setQueryData(MODULES_KEY, [{ id: MODULE, name: "Auth", status: "completed" }]);
    render(
      <QueryClientProvider client={queryClient}>
        <EntityListScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          entity={{ scope: "module", projectId: PROJECT, moduleId: MODULE }}
          moduleId={MODULE}
          peekKey={null}
          onPeekChange={vi.fn()}
          compact={false}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("calendar-entry")).toHaveAttribute("data-can-edit", "false");
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-module-completed", "true");
  });

  it("passes project standing rather than the workspace role alone", () => {
    setup({}, [project({ archived_at: "2026-01-01T00:00:00Z", member_role: 20 })]);
    expect(screen.getByTestId("list-layout")).toHaveAttribute("data-can-edit", "false");
  });

  it("shows the finished-cycle move prompt while work can move (ISS-224)", () => {
    const harness = setup({}, [project()]);
    setCycleDetail(harness, { status: "COMPLETED", backlog_issues: 2 });
    expect(screen.getByText(COMPLETED_CYCLE_NOTE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Move unfinished work" })).toBeInTheDocument();
  });

  it("still offers the move after a transfer froze the source's snapshot", () => {
    // The oracle observes the source keeping its frozen counts and the
    // button staying up, so a refetched snapshot changes nothing here.
    const harness = setup({}, [project()]);
    setCycleDetail(harness, {
      status: "COMPLETED",
      backlog_issues: 2,
      progress_snapshot: { total_issues: 2, backlog_issues: 2 },
    });
    expect(screen.getByText(COMPLETED_CYCLE_NOTE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Move unfinished work" })).toBeInTheDocument();
  });

  it("keeps the note but drops the move when no unfinished work remains", () => {
    const harness = setup({}, [project()]);
    setCycleDetail(harness, { status: "COMPLETED", backlog_issues: 0 });
    expect(screen.getByText(COMPLETED_CYCLE_NOTE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Move unfinished work" })).not.toBeInTheDocument();
  });

  it("renders no prompt for a cycle that is still active", () => {
    const harness = setup({}, [project()]);
    setCycleDetail(harness, { status: "ACTIVE", backlog_issues: 2 });
    expect(screen.queryByText(COMPLETED_CYCLE_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Move unfinished work" })).not.toBeInTheDocument();
  });

  it("opens the move dialog from the prompt and closes it again (ISS-224)", async () => {
    const user = userEvent.setup();
    const harness = setup({}, [project()]);
    setCycleDetail(harness, { status: "COMPLETED", backlog_issues: 2 });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Move unfinished work" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Move unfinished work" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("falls back to a count envelope when results are not an array", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    (globalThis as { __entityQueryClient?: QueryClient }).__entityQueryClient = queryClient;
    queryClient.setQueryData(PROJECTS_KEY, [project()]);
    queryClient.setQueryData(PREFS_KEY, {
      filters: {},
      display_filters: {},
      display_properties: {},
      rich_filters: {},
    });
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
