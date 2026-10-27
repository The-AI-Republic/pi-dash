// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view detail screen (parity/specs/views.md §7, VIEW-021–024, 028).
// The layout components themselves are exercised by the issues suites; this
// suite pins what the views page owns: the tab title, the breadcrumb trail
// and switcher, the private lock, the missing state for every unreadable
// record, and the record's stored layout, expression and order reaching the
// body.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListResponse, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation is TanStack's own tested behavior.
// The header ellipsis (NEWFRONT-636, ISS-067) reads the page address through
// useRouterState, so the stand-in answers that too; the pathname mirrors the
// suite's PARAMS below (the factory is hoisted and cannot see those consts).
const routerHarness = vi.hoisted(() => ({
  navigate: vi.fn(),
  pathname: "/acme/projects/223e4567-e89b-12d3-a456-426614174001/views/323e4567-e89b-12d3-a456-426614174002",
}));
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string; searchStr: string } }) => unknown }) =>
    select({ location: { pathname: routerHarness.pathname, searchStr: "" } }),
}));

// The issues layout root and its five bodies are heavy; the body pin here is
// which one the stored record selects and what it is handed. The issues
// suites own their internal behavior.
const bodyHarness = vi.hoisted(() => ({
  layout: "",
  expression: null as unknown,
  orderBy: "",
  peek: null as {
    peekKey: string | null;
    peekProjectId: string | null;
    onPeekChange: (request: unknown) => void;
  } | null,
}));
vi.mock("../../issues/layouts/ViewLayoutBody.js", () => ({
  ViewLayoutBody: (props: {
    layout: string;
    richExpression: unknown;
    peek?: { peekKey: string | null; peekProjectId: string | null; onPeekChange: (request: unknown) => void };
  }) => {
    bodyHarness.layout = props.layout;
    bodyHarness.expression = props.richExpression;
    bodyHarness.peek = props.peek ?? null;
    return <div data-testid="view-body" data-layout={props.layout} data-peek={props.peek?.peekKey ?? ""} />;
  },
}));
vi.mock("../../issues/layouts/IssuesLayoutRoot.js", () => ({
  IssuesLayoutRoot: (props: {
    title: string;
    totalCount: number;
    display?: { layout: string };
    offered: readonly string[];
    renderLayout: (layout: string, prefs: unknown) => React.ReactNode;
    onDisplayChange?: (layout: string) => void;
    emptyState: React.ReactNode;
    noMatchState: React.ReactNode;
    hideSwitcher?: boolean;
    headerActions?: React.ReactNode;
    displayControl?: React.ReactNode;
    filterControl?: React.ReactNode;
    filterRow?: React.ReactNode;
  }) => {
    const prefs = props.display ?? { layout: "list", orderBy: "" };
    return (
      <div
        data-testid="layout-root"
        data-offered={props.offered.join(",")}
        data-display={props.display?.layout ?? ""}
        data-hide-switcher={String(props.hideSwitcher ?? false)}
      >
        <h2>{props.title}</h2>
        <span data-testid="root-total">{props.totalCount}</span>
        <button type="button" onClick={() => props.onDisplayChange?.("calendar")}>
          switch-calendar
        </button>
        <button type="button" onClick={() => props.onDisplayChange?.("kanban")}>
          switch-board
        </button>
        <div data-testid="root-header-actions">{props.headerActions}</div>
        <div data-testid="root-display">{props.displayControl}</div>
        <div data-testid="root-filter-control">{props.filterControl}</div>
        <div data-testid="root-filter-row">{props.filterRow}</div>
        <div data-testid="root-empty">{props.totalCount === 0 ? props.emptyState : null}</div>
        <div data-testid="root-nomatch">{props.totalCount === 0 ? null : props.noMatchState}</div>
        {props.renderLayout(props.display?.layout ?? "list", prefs)}
      </div>
    );
  },
}));
vi.mock("../../issues/emptyStates/viewEmpty.js", () => ({
  SavedViewEmptyCard: (props: { canAct: boolean; onCreate: () => void }) => (
    <div data-testid="empty-card" data-can-act={String(props.canAct)}>
      View work items will appear here
      <button type="button" onClick={props.onCreate}>
        New work item
      </button>
    </div>
  ),
  loadSavedViewEmptyCard: () => Promise.resolve(),
}));
vi.mock("../../issues/components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: { open: boolean; onOpenChange: (open: boolean) => void }) => (
    <div data-testid="create-dialog" data-open={String(props.open)}>
      <button type="button" onClick={() => props.onOpenChange(false)}>
        close-create
      </button>
    </div>
  ),
}));
// The filter builder is issues-owned; this suite pins what the detail hands
// it (the record, the live display, the feature flag) and where its parts
// land, through a synchronous stand-in for the lazy boundary.
const filterHarness = vi.hoisted(
  () =>
    ({
      view: null,
      viewDisplay: null,
      viewsEnabled: undefined,
      onSavePress: null,
      onViewUpdated: null,
    }) as {
      view: unknown;
      viewDisplay: unknown;
      viewsEnabled: boolean | undefined;
      onSavePress: ((snapshot: unknown) => void) | null;
      onViewUpdated: ((view: unknown) => void) | null;
    }
);
vi.mock("../../issues/filters/rich/lazy.js", () => ({
  loadIssueFilters: () => Promise.resolve({ default: () => null }),
  preloadIssueFilters: () => {},
  LazyIssueFilters: (props: {
    view?: unknown;
    viewDisplay?: unknown;
    viewsEnabled?: boolean;
    onSavePress?: (snapshot: unknown) => void;
    onViewUpdated?: (view: unknown) => void;
    children: (parts: { control: React.ReactNode; row: React.ReactNode }) => React.ReactNode;
  }) => {
    filterHarness.view = props.view;
    filterHarness.viewDisplay = props.viewDisplay;
    filterHarness.viewsEnabled = props.viewsEnabled;
    filterHarness.onSavePress = props.onSavePress ?? null;
    filterHarness.onViewUpdated = props.onViewUpdated ?? null;
    return (
      <>
        {props.children({
          control: (
            <button type="button" data-testid="filter-toggle-stub">
              toggle
            </button>
          ),
          row: <div data-testid="filter-row-stub">row</div>,
        })}
      </>
    );
  },
}));
vi.mock("../../issues/display/DisplayControl.js", () => ({
  DisplayControl: (props: { display?: { layout: string }; onDisplayChange?: (next: { layout: string }) => void }) => (
    <button
      type="button"
      data-testid="display-control-stub"
      data-layout={props.display?.layout ?? ""}
      onClick={() => props.onDisplayChange?.({ ...(props.display as { layout: string }), layout: "calendar" })}
    >
      display
    </button>
  ),
}));
vi.mock("../dialog/CreateViewDialog.js", () => ({
  CreateViewDialog: (props: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    prefill?: { title: string; expression: unknown; display: { layout: string } };
  }) => (
    <div
      data-testid="save-view-dialog"
      data-open={String(props.open)}
      data-title={props.prefill?.title ?? ""}
      data-layout={props.prefill?.display.layout ?? ""}
      data-expression={JSON.stringify(props.prefill?.expression ?? null)}
    >
      <button type="button" onClick={() => props.onOpenChange(false)}>
        close-save
      </button>
    </div>
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { makeView, jsonResponse } from "../testing.js";
import { SavedViewScreen } from "./SavedViewScreen.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const VIEW_ID_B = "423e4567-e89b-12d3-a456-426614174005";
const PARAMS = { ws: "acme", projectId: PROJECT_ID, viewId: VIEW_ID };

interface Harness {
  project: Record<string, unknown> | null;
  projectListStatus: number;
  view: Record<string, unknown> | null;
  viewStatus: number;
  viewsStatus: number;
  views: ReturnType<typeof makeView>[];
  issueRows: IssueListResponse["results"];
  issueTotal: number;
  issueUrls: string[];
  viewCalls: number;
}

let harness: Harness;

function projectRow(overrides: Record<string, unknown> = {}): Project & Record<string, unknown> {
  return {
    id: PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    intake_count: 0,
    sort_order: null,
    logo_props: {},
    member_role: 15,
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
    estimate: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  } as Project & Record<string, unknown>;
}

function issueRow(id: string, name: string, sequence: number): IssueListResponse["results"][number] {
  return {
    id,
    name,
    state_id: null,
    sort_order: sequence,
    completed_at: null,
    priority: "urgent" as const,
    sequence_id: sequence,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-02T00:00:00Z",
    updated_at: "2026-01-03T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

function issueResponse(): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: harness.issueTotal,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: harness.issueRows.length,
    total_results: harness.issueTotal,
    results: harness.issueRows,
  };
}

beforeEach(() => {
  bodyHarness.layout = "";
  bodyHarness.expression = null;
  bodyHarness.peek = null;
  routerHarness.navigate.mockReset();
  routerHarness.navigate.mockReset();
  filterHarness.view = null;
  filterHarness.viewDisplay = null;
  filterHarness.viewsEnabled = undefined;
  filterHarness.onSavePress = null;
  filterHarness.onViewUpdated = null;
  harness = {
    project: projectRow(),
    projectListStatus: 200,
    view: makeView({
      id: VIEW_ID,
      project: PROJECT_ID,
      name: "Triage",
      display_filters: { layout: "kanban", order_by: "-created_at" },
      rich_filters: { and: [{ priority__in: "urgent" }] },
    }),
    viewStatus: 200,
    viewsStatus: 200,
    views: [
      makeView({ id: VIEW_ID, project: PROJECT_ID, name: "Triage", updated_at: "2026-02-01T00:00:00Z" }),
      makeView({ id: VIEW_ID_B, project: PROJECT_ID, name: "Later view", updated_at: "2026-01-01T00:00:00Z" }),
    ],
    issueRows: [issueRow("523e4567-e89b-12d3-a456-426614174006", "Urgent one", 1)],
    issueTotal: 1,
    issueUrls: [],
    viewCalls: 0,
  };
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (
      url.includes(`/projects/${PROJECT_ID}/views/${VIEW_ID}/`) ||
      url.includes(`/projects/${PROJECT_ID}/views/${VIEW_ID_B}/`)
    ) {
      harness.viewCalls += 1;
      if (harness.view === null) return jsonResponse(url, harness.viewStatus, {});
      return jsonResponse(url, harness.viewStatus, harness.view);
    }
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      return jsonResponse(url, harness.viewsStatus, harness.views);
    }
    if (url.includes(`/api/workspaces/acme/projects/${PROJECT_ID}/issues/`)) {
      harness.issueUrls.push(url);
      return jsonResponse(url, 200, issueResponse());
    }
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) {
      if (harness.project === null) return jsonResponse(url, 404, {});
      return jsonResponse(url, 200, harness.project);
    }
    if (url.endsWith("/api/users/me/profile/")) return jsonResponse(url, 200, { start_of_the_week: 1 });
    if (url.endsWith("/api/workspaces/acme/projects/")) {
      return jsonResponse(url, harness.projectListStatus, harness.project === null ? [] : [harness.project]);
    }
    if (url.includes("/api/workspaces/acme/projects/")) return jsonResponse(url, 200, []);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(search: React.ComponentProps<typeof SavedViewScreen>["search"] = {}): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SavedViewScreen params={PARAMS} search={search} />
    </QueryClientProvider>
  );
}

describe("SavedViewScreen", () => {
  it("renders the stored layout with the view's issues, breadcrumb and tab title", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(await screen.findByRole("heading", { name: "Triage" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Website relaunch - Triage"));
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getAllByRole("button")[0]).toHaveTextContent("Website relaunch");
    expect(within(trail).getByRole("link", { name: "Views" })).toHaveAttribute(
      "href",
      `/acme/projects/${PROJECT_ID}/views`
    );
    expect(within(trail).getByRole("button", { name: "Triage" })).toHaveTextContent("Triage");
    // The record selected the board, not the stored personal preference.
    expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "kanban");
    expect(bodyHarness.expression).toEqual({ and: [{ priority__in: "urgent" }] });
    expect(screen.getByTestId("root-total")).toHaveTextContent("1");
  });

  it("binds the peek address the view's URL carries to its layout (ISS-167)", async () => {
    renderScreen({ peekIssueId: "issue-9", peekProjectId: "other-project" });
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    const body = await screen.findByTestId("view-body");
    expect(body).toHaveAttribute("data-peek", "issue-9");
    expect(bodyHarness.peek).toEqual({
      peekKey: "issue-9",
      peekProjectId: "other-project",
      onPeekChange: expect.any(Function),
    });
  });

  it("writes a peek open with replace, the foreign project and the nesting level", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(bodyHarness.peek).not.toBeNull();
    bodyHarness.peek?.onPeekChange({ issueId: "child", projectId: "other-project", nestingLevel: 1 });
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: ".",
      replace: true,
      search: { peekIssueId: "child", peekProjectId: "other-project", peekNestingLevel: "1" },
    });
  });

  it("strips every peek param when the view's peek closes", async () => {
    renderScreen({ peekIssueId: "issue-9", peekNestingLevel: "2", layout: "compact" });
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    bodyHarness.peek?.onPeekChange(null);
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: ".", replace: true, search: { layout: "compact" } });
  });

  it("applies the stored expression and order to the issue read", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(harness.issueUrls[0]).toContain("order_by=-created_at");
    expect(decodeURIComponent(harness.issueUrls[0] ?? "")).toContain("priority__in");
  });

  it.each([403, 404, 500])("renders the missing-view state for a %s read", async (status) => {
    harness.viewStatus = status;
    harness.view = null;
    renderScreen();
    expect(await screen.findByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
    expect(screen.queryByTestId("layout-root")).toBeNull();
    expect(screen.queryByTestId("saved-view-header")).toBeNull();
    // The tab title never claims a view that does not exist.
    await waitFor(() => expect(document.title).toBe("Website relaunch"));
  });

  it("treats a blank serialized record as missing", async () => {
    harness.view = { ...makeView(), id: "", name: "" };
    renderScreen();
    expect(await screen.findByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
    expect(screen.queryByTestId("layout-root")).toBeNull();
  });

  it("returns to the project views list from the missing state", async () => {
    harness.view = null;
    harness.viewStatus = 404;
    renderScreen();
    fireEvent.click(await screen.findByRole("button", { name: /views|back|list/i }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${PROJECT_ID}/views` });
  });

  it("switches to another view from the detail switcher", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(2));
    // List order is updated-at descending, same as the list screen.
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["Triage", "Later view"]);
    await userEvent.click(screen.getByRole("option", { name: "Later view" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/projects/${PROJECT_ID}/views/${VIEW_ID_B}`,
    });
  });

  it("shows the lock for an owner-only view and hides it for a shared one", async () => {
    harness.view = { ...makeView({ id: VIEW_ID, project: PROJECT_ID, name: "Triage", access: 0 }) };
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("saved-view-lock")).toBeInTheDocument();
  });

  it("renders no lock on a shared view", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.queryByTestId("saved-view-lock")).toBeNull();
  });

  it("shows the saved-view empty card when the stored query matches nothing", async () => {
    harness.issueRows = [];
    harness.issueTotal = 0;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("empty-card")).toHaveTextContent("View work items will appear here");
  });

  it("switches the previewed layout locally without leaving the stored one", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "kanban");
    fireEvent.click(screen.getByRole("button", { name: "switch-calendar" }));
    await waitFor(() => expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "calendar"));
    // The record stays untouched until Update view persists the live copy.
    expect(harness.view).toMatchObject({ display_filters: { layout: "kanban" } });
  });

  it("seeds state grouping when the local switch moves to board (ISS-243)", async () => {
    harness.view = {
      ...makeView({ id: VIEW_ID, project: PROJECT_ID, name: "Triage" }),
      display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    };
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "switch-board" }));
    await waitFor(() =>
      expect(filterHarness.viewDisplay).toMatchObject({ layout: "kanban", groupBy: "state", subGroupBy: null })
    );
    // The record stays untouched until Update view persists the live copy.
    expect(harness.view).toMatchObject({ display_filters: { layout: "list" } });
  });

  it("hosts the filter toggle, display and add entry on an unlocked view", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-hide-switcher", "false");
    // The toggle and the row land in their slots; the row edits the record.
    expect(screen.getByTestId("filter-toggle-stub")).toBeInTheDocument();
    expect(screen.getByTestId("filter-row-stub")).toBeInTheDocument();
    expect(filterHarness.view).toMatchObject({ id: VIEW_ID, name: "Triage" });
    expect(filterHarness.viewDisplay).toMatchObject({ layout: "kanban" });
    expect(filterHarness.viewsEnabled).toBe(true);
    // Display rides both the desktop actions and the mobile slot.
    expect(screen.getAllByTestId("display-control-stub")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Add work item" })).toBeInTheDocument();
  });

  it("hides the switcher, display and filter toggle on a locked view", async () => {
    harness.view = {
      ...makeView({
        id: VIEW_ID,
        project: PROJECT_ID,
        name: "Triage",
        is_locked: true,
        display_filters: { layout: "kanban", order_by: "sort_order" },
      }),
    };
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("layout-root")).toHaveAttribute("data-hide-switcher", "true");
    expect(screen.queryByTestId("filter-toggle-stub")).not.toBeInTheDocument();
    expect(screen.queryByTestId("filter-row-stub")).not.toBeInTheDocument();
    expect(screen.queryByTestId("display-control-stub")).not.toBeInTheDocument();
    // The stored layout still renders, and the member keeps the Add entry.
    expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "kanban");
    expect(screen.getByRole("button", { name: "Add work item" })).toBeInTheDocument();
  });

  it("hides the Add entry from guests while keeping the toggle and display", async () => {
    harness.project = projectRow({ member_role: 5 });
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
    expect(screen.getByTestId("filter-toggle-stub")).toBeInTheDocument();
    expect(screen.getAllByTestId("display-control-stub")).toHaveLength(2);
  });

  it("opens the issues create dialog from the header Add entry", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    expect(await screen.findByTestId("create-dialog")).toHaveAttribute("data-open", "true");
    fireEvent.click(screen.getByRole("button", { name: "close-create" }));
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).toBeNull());
  });

  it("edits the live display copy through the Display control", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "kanban");
    fireEvent.click(screen.getAllByTestId("display-control-stub")[0]!);
    await waitFor(() => expect(screen.getByTestId("view-body")).toHaveAttribute("data-layout", "calendar"));
    // The record stays untouched until Update view persists the live copy.
    expect(harness.view).toMatchObject({ display_filters: { layout: "kanban" } });
  });

  it("opens Save as with the live snapshot and the suffixed title", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(filterHarness.onSavePress).not.toBeNull();
    const snapshot = {
      expression: { priority__in: "urgent" },
      displayFilters: { layout: "kanban" },
      displayProperties: {},
    };
    act(() => {
      filterHarness.onSavePress!(snapshot);
    });
    const dialog = await screen.findByTestId("save-view-dialog");
    expect(dialog).toHaveAttribute("data-open", "true");
    expect(dialog).toHaveAttribute("data-title", "Triage 2");
    expect(dialog).toHaveAttribute("data-layout", "kanban");
    expect(dialog).toHaveAttribute("data-expression", JSON.stringify({ priority__in: "urgent" }));
    fireEvent.click(screen.getByRole("button", { name: "close-save" }));
    await waitFor(() => expect(screen.queryByTestId("save-view-dialog")).toBeNull());
  });

  it("adopts an Update-view echo into the record and the live copy", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(filterHarness.onViewUpdated).not.toBeNull();
    const echo = {
      ...makeView({ id: VIEW_ID, project: PROJECT_ID, name: "Triage" }),
      display_filters: { layout: "calendar", order_by: "sort_order" },
      display_properties: {},
      rich_filters: { priority__in: "low" },
    };
    act(() => {
      filterHarness.onViewUpdated!(echo);
    });
    // The live copy follows the echo.
    await waitFor(() =>
      expect(screen.getAllByTestId("display-control-stub")[0]).toHaveAttribute("data-layout", "calendar")
    );
    // And the cached record the row edits is the echo.
    await waitFor(() => expect(filterHarness.view).toMatchObject({ rich_filters: { priority__in: "low" } }));
  });

  it("opens the project from the first crumb", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    fireEvent.click(await screen.findByRole("button", { name: "Website relaunch" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${PROJECT_ID}` });
  });

  it("keeps the view usable when the project row is not visible", async () => {
    harness.project = null;
    harness.issueRows = [];
    harness.issueTotal = 0;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(document.title).toBe("Triage");
    expect(screen.getByTestId("empty-card")).toHaveAttribute("data-can-act", "false");
    expect(screen.getByTestId("saved-view-header")).toBeInTheDocument();
  });

  it("survives a failed project list and a failed view list", async () => {
    harness.projectListStatus = 500;
    harness.viewsStatus = 500;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("layout-root")).toBeInTheDocument());
    expect(document.title).toBe("Triage");
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Views could not be loaded."));
  });

  it("opens the issues create dialog from the empty card and closes it", async () => {
    harness.issueRows = [];
    harness.issueTotal = 0;
    renderScreen();
    const card = await screen.findByTestId("empty-card");
    fireEvent.click(within(card).getByRole("button", { name: "New work item" }));
    expect(await screen.findByTestId("create-dialog")).toHaveAttribute("data-open", "true");
    fireEvent.click(screen.getByRole("button", { name: "close-create" }));
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).toBeNull());
  });
});
