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
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListResponse, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation is TanStack's own tested behavior.
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

// The issues layout root and its five bodies are heavy; the body pin here is
// which one the stored record selects and what it is handed. The issues
// suites own their internal behavior.
const bodyHarness = vi.hoisted(() => ({ layout: "", expression: null as unknown, orderBy: "" }));
vi.mock("../../issues/layouts/ViewLayoutBody.js", () => ({
  ViewLayoutBody: (props: { layout: string; richExpression: unknown }) => {
    bodyHarness.layout = props.layout;
    bodyHarness.expression = props.richExpression;
    return <div data-testid="view-body" data-layout={props.layout} />;
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
  }) => {
    const prefs = props.display ?? { layout: "list", orderBy: "" };
    return (
      <div data-testid="layout-root" data-offered={props.offered.join(",")} data-display={props.display?.layout ?? ""}>
        <h2>{props.title}</h2>
        <span data-testid="root-total">{props.totalCount}</span>
        <button type="button" onClick={() => props.onDisplayChange?.("calendar")}>
          switch-calendar
        </button>
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
  routerHarness.navigate.mockReset();
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
    // No write path exists in this slice; the record stays untouched.
    expect(harness.view).toMatchObject({ display_filters: { layout: "kanban" } });
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
