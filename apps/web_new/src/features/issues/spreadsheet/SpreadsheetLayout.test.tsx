// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import type { IssueListItem, IssueListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IssuesLoaderProvider } from "../loaders/loaderContext.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { buildDisplayRows, SpreadsheetLayout, useViewportWindow } from "./SpreadsheetLayout.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ID1 = "123e4567-e89b-12d3-a456-426614174000";
const ID2 = "323e4567-e89b-12d3-a456-426614174002";
const ID3 = "423e4567-e89b-12d3-a456-426614174003";
const ID4 = "523e4567-e89b-12d3-a456-426614174004";
const ENTITY = { scope: "project", projectId: PROJECT } as const;

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "spreadsheet",
    groupBy: null,
    subGroupBy: null,
    orderBy: "-created_at",
    showEmptyGroups: false,
    showSubIssues: false,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function issue(id: string, name: string, sequence: number, overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id,
    name,
    state_id: null,
    sort_order: sequence,
    completed_at: null,
    priority: "none",
    sequence_id: sequence,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function page(results: IssueListItem[], overrides: Partial<IssueListResponse> = {}): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
    ...overrides,
  };
}

interface Stub {
  calls: Array<{ url: string; method: string; body: string }>;
  pages: IssueListResponse[];
  children: Record<string, IssueListItem[]>;
}

function stub(options: Partial<Stub> = {}): Stub {
  const state: Stub = {
    calls: [],
    pages: [page([issue(ID1, "Fix login", 1), issue(ID2, "Second", 2)])],
    children: {},
    ...options,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    state.calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    let status = 200;
    let body: unknown = {};
    if (url.endsWith("/auth/get-csrf-token/")) body = { csrf_token: "csrf" };
    else if (url.includes("/sub-issues/")) {
      const parent = url.split("/issues/")[1]?.split("/")[0] ?? "";
      body = { sub_issues: state.children[parent] ?? [], state_distribution: {} };
    } else if (url.includes(`/projects/${PROJECT}/issues/`) && !url.includes("/sub-issues/")) {
      const parsed = new URL(url, "https://api.test");
      body =
        parsed.searchParams.get("cursor") === null
          ? state.pages[0]
          : page(state.pages.slice(1).flatMap((entry) => entry.results));
      status = 200;
    } else if (url.includes("/user-properties/")) {
      body = ProjectPrefsPayload;
    } else if (init.method === "PATCH") {
      status = 201;
      body = ProjectPrefsPayload;
    }
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return state;
}

const ProjectPrefsPayload = {
  id: ID3,
  workspace: ID3,
  project: PROJECT,
  user: ID3,
  filters: {},
  display_filters: { layout: "spreadsheet", group_by: null, order_by: "-created_at" },
  display_properties: {},
  rich_filters: {},
  preferences: {},
  sort_order: 1,
  created_by: null,
  updated_by: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function renderSheet(
  props: {
    prefs?: DisplayPrefs;
    canEdit?: boolean;
    hideCycle?: boolean;
    hideModule?: boolean;
    highlightedId?: string | null;
  } = {}
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({
    component: () => (
      <IssuesLoaderProvider>
        <Outlet />
      </IssuesLoaderProvider>
    ),
  });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const browseRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "browse/$issueKey",
    component: () => <p>Browse</p>,
  });
  const sheetRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/issues",
    component: () => (
      <SpreadsheetLayout
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        entity={ENTITY}
        prefs={props.prefs ?? prefs()}
        canEdit={props.canEdit ?? true}
        hideCycle={props.hideCycle ?? false}
        hideModule={props.hideModule ?? false}
        highlightedId={props.highlightedId ?? null}
      />
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([browseRoute, sheetRoute])]),
    history: createMemoryHistory({ initialEntries: [`/${SLUG}/projects/${PROJECT}/issues`] }),
    defaultPreload: false,
  });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { ...result, queryClient, router };
}

function setViewport(width: number, height: number): void {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("data-testid") === "spreadsheet-scroller") return height;
      if (this.tagName === "DIV" && this.parentElement?.getAttribute("data-testid") === "spreadsheet-scroller") {
        return height;
      }
      return 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => width });
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div><div id="menu-portal"></div>`;
  window.localStorage.clear();
  setViewport(800, 400);
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("SpreadsheetLayout", () => {
  it("renders the header order and the enabled display columns", async () => {
    stub();
    renderSheet({ prefs: prefs({ properties: { labels: false, cycle: false, modules: false } }) });
    await screen.findByRole("table");
    const headers = screen.getAllByRole("columnheader").map((header) => header.textContent);
    expect(headers[0]).toBe("Work items");
    expect(headers).not.toContain("Labels");
    expect(headers).not.toContain("Cycle");
    expect(headers).not.toContain("Modules");
    expect(headers).toContain("State");
    expect(headers).toContain("Due date");
  });

  it("hides cycle and modules when the project features are off", async () => {
    stub();
    renderSheet({ hideCycle: true, hideModule: true });
    await screen.findByRole("table");
    const headers = screen.getAllByRole("columnheader").map((header) => header.textContent);
    expect(headers).not.toContain("Cycle");
    expect(headers).not.toContain("Modules");
  });

  it("keeps the first column and header sticky and shadows the column on scroll", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    const table = screen.getByRole("table");
    expect(within(table).getAllByRole("columnheader")[0]).toHaveClass("sticky");
    expect(table.querySelector("thead")).toHaveClass("sticky");
    await waitFor(() => expect(table.querySelector(`td#issue-${ID1}`)).not.toBeNull());
    const first = table.querySelector(`td#issue-${ID1}`) as HTMLElement;
    expect(first).toHaveClass("sticky");
    expect(first.style.boxShadow).toBe("none");
    fireEvent.scroll(screen.getByTestId("spreadsheet-scroller"), { target: { scrollLeft: 100 } });
    await waitFor(() => expect(first.style.boxShadow).not.toBe("none"));
  });

  it("renders each row's identifier and title and highlights requested rows", async () => {
    stub();
    renderSheet({ highlightedId: ID2 });
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByText("Fix login")).toBeInTheDocument());
    expect(screen.getByText("Second")).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Second/ })).toHaveAttribute("data-highlight", "true");
  });

  it("offers ascending, descending and clear on a sorted header, then writes order_by", async () => {
    const state = stub();
    renderSheet();
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    fireEvent.click(within(stateHeader).getByRole("button"));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Ascending", "Descending"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    await waitFor(() => expect(state.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(window.localStorage.getItem("web_new.spreadsheet.sort")).toContain("state__name");
  });

  it("adds Clear sorting once sorted and clears to creation order", async () => {
    stub();
    renderSheet({ prefs: prefs({ orderBy: "state__name" }) });
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    fireEvent.click(within(stateHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();
  });

  it("restores a remembered created-on ascending sort after reload", async () => {
    window.localStorage.setItem(
      "web_new.spreadsheet.sort",
      JSON.stringify({ column: "created_on", direction: "ascending", orderBy: "-created_at" })
    );
    stub();
    renderSheet();
    await screen.findByRole("table");
    const createdHeader = screen.getByRole("columnheader", { name: /Created on/ });
    fireEvent.click(within(createdHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();
  });

  it("opens quick-add, submits on Enter and keeps the form open", async () => {
    const state = stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    const input = screen.getByPlaceholderText("Work item title");
    fireEvent.change(input, { target: { value: "New sheet item" } });
    fireEvent.submit(input.closest("form")!);
    await waitFor(() => expect(state.calls.some((call) => call.method === "POST")).toBe(true));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
  });

  it("closes quick-add on Escape", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    fireEvent.keyDown(screen.getByPlaceholderText("Work item title"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
  });

  it("closes quick-add on an outside pointer press", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
  });

  it("disables quick-add without edit permission", async () => {
    stub();
    renderSheet({ canEdit: false });
    await screen.findByRole("table");
    expect(screen.getByRole("button", { name: "Add work item" })).toBeDisabled();
  });

  it("loads the next page when the viewport reaches the bottom", async () => {
    const firstRows = Array.from({ length: 20 }, (_, index) => issue(ID1, `Fix login ${index}`, index + 1));
    const first = page(firstRows, { next_page_results: true, next_cursor: "next", total_count: 21 });
    const second = page([issue(ID2, "Second", 21)], { total_count: 21 });
    const state = stub({ pages: [first, second] });
    renderSheet();
    await screen.findByRole("table");
    const scroller = screen.getByTestId("spreadsheet-scroller");
    Object.defineProperty(scroller, "scrollHeight", { value: 880, configurable: true });
    Object.defineProperty(scroller, "scrollTop", { value: 472, configurable: true });
    fireEvent.scroll(scroller);
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("cursor=next"))).toBe(true));
  });

  it("expands a sub-issue inline and indents the child", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2), state_group: null };
    const state = stub({ pages: [page([parent])], children: { [ID1]: [child] } });
    renderSheet();
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByRole("button", { name: "Expand sub-work items" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items" }));
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("/sub-issues/"))).toBe(true));
    await screen.findByText("Child");
    const childCell = document.getElementById(`issue-${ID2}`) as HTMLElement;
    const spacer = childCell.querySelector('div[style*="width"]') as HTMLElement;
    expect(spacer.style.width).toBe("12px");
  });

  it("opens peek instead of expanding past three levels", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2, { parent_id: ID1, sub_issues_count: 1 }), state_group: null };
    const grandchild = { ...issue(ID3, "Grandchild", 3, { parent_id: ID2, sub_issues_count: 1 }), state_group: null };
    const greatGrandchild = {
      ...issue(ID4, "Great-grandchild", 4, { parent_id: ID3, sub_issues_count: 1 }),
      state_group: null,
    };
    stub({
      pages: [page([parent])],
      children: { [ID1]: [child], [ID2]: [grandchild], [ID3]: [greatGrandchild] },
    });
    renderSheet();
    await screen.findByRole("table");
    for (const id of [ID1, ID2, ID3]) {
      await waitFor(() =>
        expect(
          document.getElementById(`issue-${id}`)?.querySelector('button[aria-label="Expand sub-work items"]')
        ).not.toBeNull()
      );
      fireEvent.click(
        document
          .getElementById(`issue-${id}`)
          ?.querySelector('button[aria-label="Expand sub-work items"]') as HTMLElement
      );
    }
    await screen.findByText("Great-grandchild");
    const deeper = document.getElementById(`issue-${ID4}`)?.querySelector('button[aria-label="Expand sub-work items"]');
    expect(deeper).not.toBeNull();
    fireEvent.click(deeper as HTMLElement);
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
  });

  it("moves focus between cells with arrow keys and clamps at the edges", async () => {
    stub();
    renderSheet({
      prefs: prefs({
        properties: {
          labels: false,
          cycle: false,
          modules: false,
          estimate: false,
          created_on: false,
          updated_on: false,
          link: false,
          attachment_count: false,
          sub_issue_count: false,
          assignee: false,
          start_date: false,
          due_date: false,
        },
      }),
    });
    await screen.findByRole("table");
    await waitFor(() => expect(document.getElementById(`issue-${ID1}`)).not.toBeNull());
    const first = document.getElementById(`issue-${ID1}`) as HTMLElement;
    const stateCell = document.getElementById(`sheet-cell-${ID1}-state`)!;
    stateCell.focus();
    fireEvent.keyDown(stateCell, { key: "ArrowRight" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID1}-priority`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID2}-priority`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID2}-state`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(document.activeElement).toBe(stateCell);
    first.focus();
  });
});

describe("spreadsheet row helpers", () => {
  it("builds depth-first display rows only for expanded parents", () => {
    const parent = issue(ID1, "Parent", 1);
    const child = { ...issue(ID2, "Child", 2), state_group: null };
    const grandchild = issue(ID3, "Grandchild", 3);
    const rows = buildDisplayRows(
      [parent],
      new Set([ID1, ID2]),
      new Map([
        [ID1, [child]],
        [ID2, [grandchild]],
      ])
    );
    expect(rows.map((row) => [row.issue.name, row.depth])).toEqual([
      ["Parent", 0],
      ["Child", 1],
      ["Grandchild", 2],
    ]);
    expect(buildDisplayRows([parent], new Set(), new Map())).toHaveLength(1);
  });

  it("computes a stable viewport window and reports the bottom", () => {
    const node = document.createElement("div");
    Object.defineProperty(node, "clientHeight", { value: 100 });
    Object.defineProperty(node, "scrollTop", { value: 0, writable: true, configurable: true });
    function Probe(): React.ReactElement {
      const window = useViewportWindow(node, 44, 20, 1);
      return <span data-testid="window">{JSON.stringify(window)}</span>;
    }
    render(<Probe />);
    expect(screen.getByTestId("window").textContent).toBe(JSON.stringify({ start: 0, end: 5, atBottom: false }));
  });
});
