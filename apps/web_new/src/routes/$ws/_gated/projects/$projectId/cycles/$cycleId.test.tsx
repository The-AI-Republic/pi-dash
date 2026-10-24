// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail route: the trail, the shared layouts bound to the cycle
// entity, the header actions and the creation dialog (CYC-032–034).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  page: vi.fn(),
  prefs: vi.fn(),
  projects: vi.fn(),
  cycle: vi.fn(),
  roles: { current: { data: {} as Record<string, number>, isPending: false } },
  projectRow: { current: { data: { timezone: "America/New_York" } as { timezone?: string } | undefined } },
  sidebarCollapsed: { current: false },
  setSidebarCollapsed: vi.fn(),
  layout: { current: "list" },
  filter: { current: null as null | Record<string, unknown> },
  parsed: { current: [] as unknown[] },
  overlayCreate: vi.fn(),
  overlayAdd: vi.fn(),
  clearFilters: vi.fn(),
  loadKanban: vi.fn(async () => ({ default: () => null })),
  loadList: vi.fn(async () => ({ default: () => null })),
  loadLeaves: vi.fn(async () => undefined),
  primeLeaves: vi.fn(),
  preloadFilters: vi.fn(),
}));

vi.mock("../../../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));
vi.mock("../../../../../../features/cycles/api/cycleDetail.js", () => ({ cycleDetailQueryOptions: mocks.cycle }));
vi.mock("../../../../../../features/cycles/components/CycleDetailGate.js", () => ({
  CycleDetailGate: (props: Record<string, unknown>) => (
    <div
      data-testid="detail-gate"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
      data-cycle={String(props.cycleId ?? "")}
    >
      {props.children as never}
    </div>
  ),
}));
vi.mock("../../../../../../features/cycles/components/CycleMemberDates.js", () => ({
  CycleMemberDates: (props: Record<string, unknown>) => (
    <div
      data-testid="member-dates"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
      data-cycle={String(props.cycleId ?? "")}
    />
  ),
}));
vi.mock("../../../../../../features/cycles/components/CycleDetailCrumbs.js", () => ({
  CycleDetailCrumbs: (props: Record<string, unknown>) => (
    <div
      data-testid="detail-crumbs"
      data-project={String(props.projectName ?? "")}
      data-cycle={String(props.cycleName ?? "")}
      data-status={String(props.cycleStatus ?? "")}
    >
      {props.narrowActions as never}
    </div>
  ),
}));
vi.mock("../../../../../../features/cycles/api/cycleSidebarState.js", () => ({
  useCycleSidebarCollapsed: () => [mocks.sidebarCollapsed.current, mocks.setSidebarCollapsed],
}));
vi.mock("../../../../../../features/cycles/components/CycleDetailSidebar.js", () => ({
  CycleDetailSidebar: (props: Record<string, unknown>) => (
    <div
      data-testid="detail-sidebar"
      data-cycle={String((props.cycle as { name: string }).name ?? "")}
      data-timezone={String(props.projectTimeZone ?? "")}
      data-estimates={String(props.estimatesEnabled ?? false)}
      data-editable={String(props.editable ?? false)}
    >
      <button type="button" onClick={props.onClose as () => void}>
        close sidebar
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../features/cycles/components/CycleSidebarToggle.js", () => ({
  CycleSidebarToggle: (props: Record<string, unknown>) => (
    <button
      type="button"
      data-testid={String(props.testId ?? "cycle-sidebar-toggle")}
      data-collapsed={String(props.collapsed ?? false)}
      onClick={props.onToggle as () => void}
    >
      toggle
    </button>
  ),
}));
vi.mock("../../../../../../features/auth/queries.js", () => ({
  useProjectRoles: () => mocks.roles.current,
  useProjectRow: () => mocks.projectRow.current,
}));
vi.mock("../../../../../../features/projects/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../../../features/projects/index.js")>()),
  projectsQueryOptions: mocks.projects,
}));
vi.mock("../../../../../../features/issues/display/DisplayControl.js", () => ({
  DisplayControl: (props: Record<string, unknown>) => (
    <div
      data-testid="display-control"
      data-scope={String((props.entity as { scope: string }).scope)}
      data-offered={(props.offered as string[]).join(",")}
      data-hide-cycle={String(props.hideCycle ?? false)}
      data-hide-module={String(props.hideModule ?? false)}
    />
  ),
}));
vi.mock("../../../../../../features/issues/layouts/IssuesLayoutRoot.js", () => ({
  IssuesLayoutRoot: (props: Record<string, unknown>) => (
    <div
      data-testid="layout-root"
      data-title={String(props.title ?? "")}
      data-total={String(props.totalCount ?? "")}
      data-result={String(props.resultCount ?? "")}
      data-offered={(props.offered as string[]).join(",")}
    >
      {props.headerActions as never}
      {props.displayControl as never}
      {props.filterControl as never}
      {props.filterRow as never}
      {props.analyticsControl as never}
      {props.emptyState as never}
      {(props.renderLayout as (layout: string, prefs: Record<string, unknown>) => React.ReactNode)(
        mocks.layout.current,
        { layout: mocks.layout.current }
      )}
    </div>
  ),
}));
vi.mock("../../../../../../features/issues/prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("../../../../../../features/issues/list/pages.js", () => ({ listPageQueryOptions: mocks.page }));
vi.mock("../../../../../../features/issues/components/lazyLeaves.js", () => ({
  ResolvedAnalyticsEntry: (props: Record<string, unknown>) => (
    <div
      data-testid="analytics-entry"
      data-cycle={String((props.cycle as { name: string } | undefined)?.name ?? "")}
      data-narrow={String(props.narrowGuestAccess ?? false)}
      data-total={String(props.totalCount ?? "")}
      data-rows={String((props.rows as unknown[]).length)}
    />
  ),
  loadIssueListLeaves: mocks.loadLeaves,
  primeIssueListLeaves: mocks.primeLeaves,
}));
vi.mock("../../../../../../features/issues/list/lazy.js", () => ({
  loadListLayout: mocks.loadList,
  resolvedListLayout: () => (props: Record<string, unknown>) => (
    <div
      data-testid="list-layout"
      data-can-edit={String(props.canEdit ?? false)}
      data-completed={String(props.completedCycle ?? false)}
      data-peek={String(props.peekKey ?? "")}
      data-compact={String(props.compact ?? false)}
      data-cycle={String(props.cycleId ?? "")}
    >
      <button
        type="button"
        onClick={() =>
          (props.onRegisterOverlays as (overlays: unknown) => void)({
            openCreate: mocks.overlayCreate,
            openAddExisting: mocks.overlayAdd,
          })
        }
      >
        register
      </button>
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)("WEB-1")}>
        open
      </button>
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)(null)}>
        close
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../features/issues/kanban/lazy.js", () => ({
  loadKanbanLayout: mocks.loadKanban,
  resolvedKanbanLayout: () => (props: Record<string, unknown>) => (
    <div
      data-testid="kanban-layout"
      data-can-edit={String(props.canEdit ?? false)}
      data-completed={String(props.completedCycle ?? false)}
      data-cycle={String(props.cycleId ?? "")}
      data-peek={String(props.peekKey ?? "")}
    />
  ),
}));
vi.mock("../../../../../../features/issues/emptyStates/cards.js", () => ({
  CycleIssuesEmpty: (props: Record<string, unknown>) => (
    <div
      data-testid="cycle-empty"
      data-completed={String(props.completed ?? false)}
      data-can-act={String(props.canAct ?? false)}
    >
      <button type="button" onClick={props.onCreate as () => void}>
        create
      </button>
      <button type="button" onClick={props.onAddExisting as () => void}>
        add
      </button>
    </div>
  ),
  FilteredEmptyCard: (props: Record<string, unknown>) => (
    <div data-testid="filtered-empty" data-context={String(props.context ?? "")}>
      <button type="button" onClick={props.onClear as () => void}>
        clear
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../features/issues/filters/rich/expression.js", () => ({
  parseRichExpression: () => mocks.parsed.current,
  isConditionComplete: () => true,
}));
vi.mock("../../../../../../features/issues/filters/rich/lazy.js", () => ({
  LazyIssueFilters: (props: Record<string, unknown>) => (
    <div data-testid="issue-filters" data-views={String(props.viewsEnabled ?? false)}>
      {(props.children as (filters: unknown) => React.ReactNode)(mocks.filter.current)}
    </div>
  ),
  preloadIssueFilters: mocks.preloadFilters,
}));
vi.mock("../../../../../../features/cycles/components/CyclePaletteEntries.js", () => ({
  CyclePaletteEntries: (props: Record<string, unknown>) => (
    <div
      data-testid="palette-entries"
      data-cycle={String((props.cycle as { id?: unknown } | undefined)?.id ?? "")}
      data-role={String(props.memberRole ?? "")}
      data-archived={String(props.projectArchived ?? false)}
    />
  ),
}));
vi.mock("../../../../../../features/issues/calendar/CalendarEntry.js", () => ({
  CalendarEntry: (props: Record<string, unknown>) => (
    <div
      data-testid="calendar-layout"
      data-cycle={String((props.membership as { cycleId: string }).cycleId)}
      data-can-edit={String(props.canEdit ?? false)}
      data-add={String(props.canAddExisting ?? false)}
    />
  ),
}));
vi.mock("../../../../../../features/issues/spreadsheet/SpreadsheetLayout.js", () => ({
  SpreadsheetLayout: (props: Record<string, unknown>) => (
    <div
      data-testid="spreadsheet-layout"
      data-scope={String((props.entity as { scope: string }).scope)}
      data-hide-estimate={String(props.hideEstimate ?? false)}
    />
  ),
}));
vi.mock("../../../../../../features/issues/gantt/GanttTimeline.js", () => ({
  GanttTimeline: (props: Record<string, unknown>) => (
    <div
      data-testid="gantt-layout"
      data-completed={String(props.completedCycle ?? false)}
      data-count={String((props.issues as unknown[]).length)}
    />
  ),
}));
vi.mock("../../../../../../features/issues/components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: Record<string, unknown>) =>
    props.open === true ? (
      <div
        data-testid="create-dialog"
        data-cycle={String((props.context as { cycleId: string }).cycleId)}
        data-project={String(props.initialProjectId ?? "")}
        data-projects={(props.projects as { id: string }[]).map((entry) => entry.id).join(",")}
      >
        <button type="button" onClick={() => (props.onOpenChange as (open: boolean) => void)(false)}>
          close create
        </button>
      </div>
    ) : null,
}));

import { ApiError } from "@pidash/api-client";

import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { Route } from "./$cycleId.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function projectSeed(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    member_role: 15,
    archived_at: null,
    cycle_view: true,
    module_view: true,
    issue_views_view: true,
    estimate: null,
    ...overrides,
  };
}

function cycleSeed(overrides: Record<string, unknown> = {}) {
  return { id: CYCLE, name: "Sprint 1", status: "CURRENT", ...overrides };
}

function pageSeed(overrides: Record<string, unknown> = {}) {
  return {
    total_count: 2,
    results: [
      { id: "issue-1", state_id: null },
      { id: "issue-2", state_id: null },
    ],
    count: 2,
    grouped_by: null,
    sub_grouped_by: null,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    total_results: 2,
    ...overrides,
  };
}

function renderRoute(search: Record<string, unknown> = {}) {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT, cycleId: CYCLE } as never);
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
  mocks.layout.current = "list";
  mocks.filter.current = null;
  mocks.parsed.current = [];
  mocks.roles.current = { data: { [PROJECT]: 15 }, isPending: false };
  mocks.projectRow.current = { data: { timezone: "America/New_York" } };
  mocks.sidebarCollapsed.current = false;
  mocks.page.mockReturnValue({ queryKey: ["page"], queryFn: async () => pageSeed() });
  mocks.prefs.mockReturnValue({
    queryKey: ["prefs"],
    queryFn: async () => ({ filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} }),
  });
  mocks.projects.mockReturnValue({ queryKey: ["projects"], queryFn: async () => [projectSeed()] });
  mocks.cycle.mockReturnValue({ queryKey: ["cycle"], queryFn: async () => cycleSeed() });
});

describe("cycle detail route", () => {
  it("mounts the trail, the dates and the layout root for the cycle", async () => {
    renderRoute();
    const gate = await screen.findByTestId("detail-gate");
    expect(gate).toHaveAttribute("data-workspace", "acme");
    expect(gate).toHaveAttribute("data-project", PROJECT);
    expect(gate).toHaveAttribute("data-cycle", CYCLE);
    expect(await screen.findByTestId("member-dates")).toBeInTheDocument();
    const crumbs = await screen.findByTestId("detail-crumbs");
    expect(crumbs).toHaveAttribute("data-project", "Website");
    expect(crumbs).toHaveAttribute("data-cycle", "Sprint 1");
    expect(crumbs).toHaveAttribute("data-status", "CURRENT");
    const root = await screen.findByTestId("layout-root");
    expect(root).toHaveAttribute("data-title", "Sprint 1");
    expect(root).toHaveAttribute("data-total", "2");
    expect(root).toHaveAttribute("data-result", "2");
    expect(root).toHaveAttribute("data-offered", "list,kanban,calendar,spreadsheet,gantt_chart");
    expect(await screen.findByTestId("list-layout")).toBeInTheDocument();
  });

  it("passes peek and compact from the search state", async () => {
    renderRoute({ peekIssueId: "WEB-7", layout: "compact" });
    const layout = await screen.findByTestId("list-layout");
    expect(layout).toHaveAttribute("data-peek", "WEB-7");
    expect(layout).toHaveAttribute("data-compact", "true");
    expect(layout).toHaveAttribute("data-cycle", CYCLE);
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

  it("counts the badge and hides it on an empty cycle", async () => {
    const first = renderRoute();
    expect(await screen.findByTestId("cycle-detail-badge")).toHaveTextContent("2");
    first.unmount();
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ total_count: 0, results: [], count: 0 }),
    });
    renderRoute();
    expect(await screen.findByTestId("layout-root")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-detail-badge")).not.toBeInTheDocument();
  });

  it("offers creation to members and files the dialog into the cycle", async () => {
    renderRoute();
    fireEvent.click(await screen.findByRole("button", { name: "Add work item" }));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-cycle", CYCLE);
    expect(dialog).toHaveAttribute("data-project", PROJECT);
    expect(dialog).toHaveAttribute("data-projects", PROJECT);
    fireEvent.click(screen.getByRole("button", { name: "close create" }));
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
  });

  it("hides creation from guests, outsiders and resolving roles", async () => {
    mocks.roles.current = { data: { [PROJECT]: 5 }, isPending: false };
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
  });

  it("hides creation while the role map has no entry", async () => {
    mocks.roles.current = { data: {}, isPending: false };
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
  });

  it("hides creation while the roles resolve", async () => {
    mocks.roles.current = { data: undefined as never, isPending: true };
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
  });

  it("mounts the cycle-scoped analytics entry with the narrow trigger", async () => {
    renderRoute();
    const entry = await screen.findByTestId("analytics-entry");
    expect(entry).toHaveAttribute("data-cycle", "Sprint 1");
    expect(entry).toHaveAttribute("data-narrow", "true");
    expect(entry).toHaveAttribute("data-total", "2");
    expect(entry).toHaveAttribute("data-rows", "2");
  });

  it("mounts the display control for the cycle entity", async () => {
    renderRoute();
    // The same node rides the desktop actions and the narrow bar.
    const controls = await screen.findAllByTestId("display-control");
    expect(controls).toHaveLength(2);
    expect(controls[0]).toHaveAttribute("data-scope", "cycle");
    expect(controls[0]).toHaveAttribute("data-offered", "list,kanban,calendar,spreadsheet,gantt_chart");
    expect(controls[0]).toHaveAttribute("data-hide-cycle", "false");
    expect(controls[0]).toHaveAttribute("data-hide-module", "false");
  });

  it("hides the cycle and module display flags the project switches off", async () => {
    mocks.projects.mockReturnValue({
      queryKey: ["projects"],
      queryFn: async () => [projectSeed({ cycle_view: false, module_view: false })],
    });
    renderRoute();
    const controls = await screen.findAllByTestId("display-control");
    expect(controls[0]).toHaveAttribute("data-hide-cycle", "true");
    expect(controls[0]).toHaveAttribute("data-hide-module", "true");
  });

  it("renders the board, calendar, sheet and timeline from the stored layout", async () => {
    mocks.layout.current = "kanban";
    mocks.filter.current = { control: null, row: null, hasCompleteCondition: false, addedThisVisit: false };
    renderRoute({ peekIssueId: "WEB-7" });
    const board = await screen.findByTestId("kanban-layout");
    expect(board).toHaveAttribute("data-can-edit", "true");
    expect(board).toHaveAttribute("data-completed", "false");
    expect(board).toHaveAttribute("data-cycle", CYCLE);
    expect(board).toHaveAttribute("data-peek", "WEB-7");
  });

  it("renders the calendar with the cycle membership", async () => {
    mocks.layout.current = "calendar";
    renderRoute();
    const calendar = await screen.findByTestId("calendar-layout");
    expect(calendar).toHaveAttribute("data-cycle", CYCLE);
    expect(calendar).toHaveAttribute("data-can-edit", "true");
    expect(calendar).toHaveAttribute("data-add", "true");
  });

  it("renders the spreadsheet for the cycle entity", async () => {
    mocks.layout.current = "spreadsheet";
    renderRoute();
    const sheet = await screen.findByTestId("spreadsheet-layout");
    expect(sheet).toHaveAttribute("data-scope", "cycle");
    expect(sheet).toHaveAttribute("data-hide-estimate", "true");
  });

  it("renders the timeline over the flat rows", async () => {
    mocks.layout.current = "gantt_chart";
    renderRoute();
    const timeline = await screen.findByTestId("gantt-layout");
    expect(timeline).toHaveAttribute("data-completed", "false");
    expect(timeline).toHaveAttribute("data-count", "2");
  });

  it("reads grouped pages as empty rows with the server count", async () => {
    mocks.layout.current = "gantt_chart";
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ results: { Backlog: [] }, count: 5, total_count: 5 }),
    });
    renderRoute();
    expect(await screen.findByTestId("gantt-layout")).toHaveAttribute("data-count", "0");
    expect(await screen.findByTestId("analytics-entry")).toHaveAttribute("data-rows", "0");
    expect(await screen.findByTestId("layout-root")).toHaveAttribute("data-result", "5");
  });

  it("renders the project-not-found section without the project", async () => {
    mocks.projects.mockReturnValue({ queryKey: ["projects"], queryFn: async () => [] });
    renderRoute();
    expect(await screen.findByText("Project not found")).toBeInTheDocument();
    expect(screen.queryByTestId("layout-root")).not.toBeInTheDocument();
  });

  it("locks a finished cycle to read-only but keeps the badge and analytics", async () => {
    mocks.cycle.mockReturnValue({ queryKey: ["cycle"], queryFn: async () => cycleSeed({ status: "COMPLETED" }) });
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toHaveAttribute("data-can-edit", "false");
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
    expect(screen.getByTestId("cycle-detail-badge")).toHaveTextContent("2");
    expect(screen.getByTestId("analytics-entry")).toBeInTheDocument();
  });

  it("locks an archived project to read-only", async () => {
    mocks.projects.mockReturnValue({
      queryKey: ["projects"],
      queryFn: async () => [projectSeed({ archived_at: "2026-01-01T00:00:00Z" })],
    });
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toHaveAttribute("data-can-edit", "false");
  });

  it("registers the cycle palette entries for the page", async () => {
    renderRoute();
    const entries = await screen.findByTestId("palette-entries");
    expect(entries).toHaveAttribute("data-cycle", CYCLE);
    expect(entries).toHaveAttribute("data-role", "15");
    expect(entries).toHaveAttribute("data-archived", "false");
  });

  it("carries the finished notice with the transfer action on completed cycles holding work", async () => {
    mocks.cycle.mockReturnValue({
      queryKey: ["cycle"],
      queryFn: async () =>
        cycleSeed({
          status: "COMPLETED",
          total_issues: 3,
          completed_issues: 1,
          cancelled_issues: 0,
          progress_snapshot: null,
        }),
    });
    renderRoute();
    expect(await screen.findByRole("region", { name: "Finished cycle" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Transfer work items" })).toBeInTheDocument();
  });

  it("keeps the notice but drops the action without work, without a role, or mid-cycle", async () => {
    mocks.cycle.mockReturnValue({
      queryKey: ["cycle"],
      queryFn: async () =>
        cycleSeed({ status: "COMPLETED", total_issues: 1, completed_issues: 1, cancelled_issues: 0 }),
    });
    const { unmount } = renderRoute();
    expect(await screen.findByRole("region", { name: "Finished cycle" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Transfer work items" })).not.toBeInTheDocument();
    unmount();

    mocks.roles.current = { data: { [PROJECT]: 5 }, isPending: false };
    mocks.cycle.mockReturnValue({
      queryKey: ["cycle"],
      queryFn: async () =>
        cycleSeed({ status: "COMPLETED", total_issues: 3, completed_issues: 1, cancelled_issues: 0 }),
    });
    const guest = renderRoute();
    expect(await screen.findByRole("region", { name: "Finished cycle" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Transfer work items" })).not.toBeInTheDocument();
    guest.unmount();

    mocks.roles.current = { data: { [PROJECT]: 15 }, isPending: false };
    mocks.cycle.mockReturnValue({ queryKey: ["cycle"], queryFn: async () => cycleSeed({ status: "CURRENT" }) });
    renderRoute();
    expect(await screen.findByTestId("list-layout")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Finished cycle" })).not.toBeInTheDocument();
  });

  it("clears through the visit-scoped filtered card", async () => {
    mocks.filter.current = {
      control: null,
      row: null,
      hasCompleteCondition: true,
      addedThisVisit: true,
      clearFilters: mocks.clearFilters,
    };
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ total_count: 0, results: [], count: 0 }),
    });
    renderRoute();
    expect(await screen.findByTestId("filtered-empty")).toHaveAttribute("data-context", "cycle");
    fireEvent.click(screen.getByRole("button", { name: "clear" }));
    expect(mocks.clearFilters).toHaveBeenCalled();
  });

  it("keeps the stored filtered card inert while the filters load", async () => {
    mocks.parsed.current = [{}];
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ total_count: 0, results: [], count: 0 }),
    });
    renderRoute();
    expect(await screen.findByTestId("filtered-empty")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "clear" }));
    expect(mocks.clearFilters).not.toHaveBeenCalled();
  });

  it("runs the list overlays once the layout registers them", async () => {
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ total_count: 0, results: [], count: 0 }),
    });
    renderRoute();
    expect(await screen.findByTestId("cycle-empty")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "register" }));
    fireEvent.click(screen.getByRole("button", { name: "create" }));
    fireEvent.click(screen.getByRole("button", { name: "add" }));
    expect(mocks.overlayCreate).toHaveBeenCalled();
    expect(mocks.overlayAdd).toHaveBeenCalled();
  });

  it("tolerates overlay calls before any layout registers", async () => {
    mocks.layout.current = "kanban";
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => pageSeed({ total_count: 0, results: [], count: 0 }),
    });
    renderRoute();
    expect(await screen.findByTestId("cycle-empty")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "create" }));
    fireEvent.click(screen.getByRole("button", { name: "add" }));
    expect(mocks.overlayCreate).not.toHaveBeenCalled();
    expect(mocks.overlayAdd).not.toHaveBeenCalled();
  });

  it("prefetches the cycle reads and the board and leaf chunks", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await beforeLoad({
      context: { queryClient, apiClient: { stub: true } },
      params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
    } as never);
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE },
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: true,
    });
    expect(mocks.prefs).toHaveBeenCalledWith(expect.anything(), "acme", {
      scope: "cycle",
      projectId: PROJECT,
      cycleId: CYCLE,
    });
    expect(mocks.projects).toHaveBeenCalledWith(expect.anything(), "acme");
    expect(mocks.cycle).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, CYCLE);
    expect(mocks.loadKanban).toHaveBeenCalled();
    expect(mocks.loadLeaves).toHaveBeenCalled();
    expect(mocks.loadList).toHaveBeenCalled();
  }, 20000);

  it("lets a missing cycle through the prefetch to the gone-away gate", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => {
        throw new ApiError({ status: 404, code: "http", message: "missing" });
      },
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
      } as never)
    ).resolves.toBe(undefined);
  });

  it("lets a missing cycle detail through the prefetch as well", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    mocks.cycle.mockReturnValue({
      queryKey: ["cycle"],
      queryFn: async () => {
        throw new ApiError({ status: 404, code: "http", message: "missing" });
      },
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
      } as never)
    ).resolves.toBe(undefined);
  });

  it("rethrows non-404 prefetch failures", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const failure = new ApiError({ status: 500, code: "http", message: "boom" });
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => {
        throw failure;
      },
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
      } as never)
    ).rejects.toBe(failure);
  });

  it("rethrows non-API prefetch failures", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const failure = new Error("boom");
    mocks.page.mockReturnValue({
      queryKey: ["page"],
      queryFn: async () => {
        throw failure;
      },
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await expect(
      beforeLoad({
        context: { queryClient, apiClient: { stub: true } },
        params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
      } as never)
    ).rejects.toBe(failure);
  });

  it("validates search through the issue search parser", () => {
    const validateSearch = Route.options.validateSearch as unknown as (input: Record<string, unknown>) => unknown;
    expect(validateSearch({})).toEqual(parseIssueSearch({}));
  });

  it("renders the list skeleton while pending", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("missing pendingComponent");
    render(React.createElement(Pending));
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("renders the load error with a retry", () => {
    const ErrorComponent = Route.options.errorComponent;
    if (!ErrorComponent) throw new Error("missing errorComponent");
    const reset = vi.fn();
    const { rerender } = render(React.createElement(ErrorComponent, { error: new Error("down"), reset }));
    expect(screen.getByText("Could not load cycle details")).toBeInTheDocument();
    expect(screen.getByText("down")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(reset).toHaveBeenCalled();
    rerender(React.createElement(ErrorComponent, { error: "offline", reset }));
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });

  it("mounts the sidebar expanded with its wiring and both toggles", async () => {
    renderRoute();
    const sidebar = await screen.findByTestId("detail-sidebar");
    expect(sidebar).toHaveAttribute("data-cycle", "Sprint 1");
    expect(sidebar).toHaveAttribute("data-timezone", "America/New_York");
    expect(sidebar).toHaveAttribute("data-estimates", "false");
    expect(sidebar).toHaveAttribute("data-editable", "true");
    expect(await screen.findByTestId("cycle-sidebar-toggle")).toHaveAttribute("data-collapsed", "false");
    expect(await screen.findByTestId("cycle-sidebar-toggle-narrow")).toHaveAttribute("data-collapsed", "false");
  });

  it("hides the sidebar while collapsed", async () => {
    mocks.sidebarCollapsed.current = true;
    renderRoute();
    expect(await screen.findByTestId("layout-root")).toBeInTheDocument();
    expect(screen.queryByTestId("detail-sidebar")).not.toBeInTheDocument();
    expect(await screen.findByTestId("cycle-sidebar-toggle")).toHaveAttribute("data-collapsed", "true");
  });

  it("flips collapse from the toggles and the panel close", async () => {
    renderRoute();
    fireEvent.click(await screen.findByTestId("cycle-sidebar-toggle"));
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(true);
    fireEvent.click(await screen.findByTestId("cycle-sidebar-toggle-narrow"));
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "close sidebar" }));
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(true);
    expect(mocks.setSidebarCollapsed).toHaveBeenCalledTimes(3);
  });

  it("enables estimates when the project carries an estimate system", async () => {
    mocks.projects.mockReturnValue({
      queryKey: ["projects"],
      queryFn: async () => [projectSeed({ estimate: "eid-1" })],
    });
    renderRoute();
    expect(await screen.findByTestId("detail-sidebar")).toHaveAttribute("data-estimates", "true");
  });

  it("falls back to UTC without a project row or timezone", async () => {
    mocks.projectRow.current = { data: undefined };
    const first = renderRoute();
    expect(await screen.findByTestId("detail-sidebar")).toHaveAttribute("data-timezone", "UTC");
    first.unmount();
    mocks.projectRow.current = { data: {} };
    renderRoute();
    expect(await screen.findByTestId("detail-sidebar")).toHaveAttribute("data-timezone", "UTC");
  });
});
